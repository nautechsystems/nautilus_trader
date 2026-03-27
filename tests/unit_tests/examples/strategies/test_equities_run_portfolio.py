from __future__ import annotations

import json
import time
import tomllib
from pathlib import Path
from typing import Any
from unittest.mock import MagicMock

import pytest

from flux.common.account_projection import ProfileAccountProviderBinding
from flux.common.keys import FluxRedisKeys
from flux.common.portfolio_inventory import StrategyInventoryComponent
from flux.common.portfolio_inventory import decode_portfolio_inventory
from flux.common.portfolio_inventory import encode_component
from flux.runners.live.hyperliquid_account import ResolvedHyperliquidUser
from flux.runners.equities.run_portfolio import EquitiesPortfolioAggregator
from flux.runners.equities.run_portfolio import _equities_strategy_ids
from flux.runners.equities.run_portfolio import _portfolio_base_assets
from flux.runners.equities.run_portfolio import _required_strategy_ids
from flux.runners.equities.run_portfolio import _strategy_ids_by_asset
from flux.runners.shared.portfolio_runner import parse_required_strategy_ids
from flux.runners.shared.portfolio_runner import parse_strategy_ids
from flux.runners.shared.profile_accounts import build_profile_account_provider_bindings
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from nautilus_trader.core import nautilus_pyo3

CORE_PROD_STRATEGY_IDS = (
    "aapl_tradexyz_maker",
    "aapl_tradexyz_taker",
    "amd_tradexyz_maker",
    "amd_tradexyz_taker",
    "amzn_tradexyz_maker",
    "amzn_tradexyz_taker",
    "googl_tradexyz_maker",
    "googl_tradexyz_taker",
    "meta_tradexyz_maker",
    "meta_tradexyz_taker",
    "msft_tradexyz_maker",
    "msft_tradexyz_taker",
    "nvda_tradexyz_maker",
    "nvda_tradexyz_taker",
    "orcl_tradexyz_maker",
    "orcl_tradexyz_taker",
    "pltr_tradexyz_maker",
    "pltr_tradexyz_taker",
    "tsla_tradexyz_maker",
    "tsla_tradexyz_taker",
)
CORE_PROD_STRATEGY_IDS_BY_ASSET = {
    "AAPL": ("aapl_tradexyz_maker", "aapl_tradexyz_taker"),
    "AMD": ("amd_tradexyz_maker", "amd_tradexyz_taker"),
    "AMZN": ("amzn_tradexyz_maker", "amzn_tradexyz_taker"),
    "GOOGL": ("googl_tradexyz_maker", "googl_tradexyz_taker"),
    "META": ("meta_tradexyz_maker", "meta_tradexyz_taker"),
    "MSFT": ("msft_tradexyz_maker", "msft_tradexyz_taker"),
    "NVDA": ("nvda_tradexyz_maker", "nvda_tradexyz_taker"),
    "ORCL": ("orcl_tradexyz_maker", "orcl_tradexyz_taker"),
    "PLTR": ("pltr_tradexyz_maker", "pltr_tradexyz_taker"),
    "TSLA": ("tsla_tradexyz_maker", "tsla_tradexyz_taker"),
}


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _load_toml(path: Path) -> dict:
    return tomllib.load(path.open("rb"))


class _FakePipeline:
    def __init__(self, redis_client: _FakeRedis) -> None:
        self._redis = redis_client
        self._keys: list[str] = []

    def get(self, key: str) -> _FakePipeline:
        self._keys.append(key)
        return self

    def execute(self) -> list[bytes | None]:
        return [self._redis.get(key) for key in self._keys]


class _FakeRedis:
    def __init__(self, values: dict[str, bytes | None] | None = None) -> None:
        self.values = dict(values or {})
        self.published: list[tuple[str, str]] = []
        self.closed = False

    def get(self, key: str) -> bytes | None:
        return self.values.get(key)

    def set(self, key: str, value: str | bytes) -> bool:
        self.values[key] = value.encode() if isinstance(value, str) else value
        return True

    def publish(self, channel: str, message: str) -> int:
        self.published.append((channel, message))
        return 1

    def pipeline(self, transaction: bool = False) -> _FakePipeline:
        _ = transaction
        return _FakePipeline(self)

    def close(self) -> None:
        self.closed = True


class _LegacyConnectionPool:
    def __init__(self) -> None:
        self.disconnect_calls: list[bool] = []

    def disconnect(self, inuse_connections: bool = True) -> None:
        self.disconnect_calls.append(inuse_connections)


class _LegacyDisconnectRedis(_FakeRedis):
    def __init__(self) -> None:
        super().__init__()
        self.connection_pool = _LegacyConnectionPool()


class _CountingAccountProjectionProvider:
    def __init__(
        self,
        *,
        rows: list[dict[str, Any]],
        totals: dict[str, Any] | None = None,
    ) -> None:
        self._rows = rows
        self._totals = totals or {}
        self.refresh_calls = 0

    def refresh(self) -> None:
        self.refresh_calls += 1

    def snapshot(self) -> dict[str, Any] | None:
        return {
            "rows": list(self._rows),
            "totals": dict(self._totals),
        }


def _strategy_contract(strategy_id: str, *, reference_account_scope_id: str) -> dict[str, str]:
    return {
        "strategy_id": strategy_id,
        "portfolio_asset_id": strategy_id.split("_", maxsplit=1)[0].upper(),
        "maker_instrument_id": f"xyz:{strategy_id.upper()}-USD-PERP.HYPERLIQUID",
        "reference_instrument_id": f"{strategy_id.upper()}.NASDAQ",
        "execution_account_scope_id": "hyperliquid.xyz.main",
        "reference_account_scope_id": reference_account_scope_id,
        "hedge_account_scope_id": "ibkr.hedge.main",
    }


def _account_scopes() -> list[dict[str, object]]:
    return [
        {
            "scope_id": "hyperliquid.xyz.main",
            "provider": "hyperliquid",
            "venue": "HYPERLIQUID",
            "private_key_env": "TRADE_XYZ_AGENT_PK",
            "account_address_env": "TRADE_XYZ_ACCOUNT_ADDRESS",
            "vault_address_env": "TRADE_XYZ_VAULT_ADDRESS",
            "dex": "xyz",
            "testnet": False,
        },
        {
            "scope_id": "ibkr.reference.main",
            "provider": "ibkr",
            "venue": "IBKR",
            "ibg_host": "127.0.0.1",
            "ibg_port": 4002,
            "ibg_client_id": 7,
            "dockerized_gateway": {
                "trading_mode": "live",
                "read_only_api": True,
            },
        },
        {
            "scope_id": "ibkr.hedge.main",
            "provider": "ibkr",
            "venue": "IBKR",
            "ibg_host": "127.0.0.1",
            "ibg_port": 4002,
            "ibg_client_id": 8,
            "dockerized_gateway": {
                "trading_mode": "live",
                "read_only_api": True,
            },
        },
    ]


def test_equities_strategy_ids_requires_non_empty_allowlist() -> None:
    with pytest.raises(ValueError, match="non-empty"):
        _equities_strategy_ids({})


def test_required_strategy_ids_falls_back_to_allowlist() -> None:
    allowlist = ["aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"]

    assert _required_strategy_ids({}, fallback=allowlist) == allowlist


def test_equities_portfolio_allowlist_uses_shared_parser() -> None:
    descriptor = get_strategy_set_descriptor("equities")

    assert parse_strategy_ids(
        {"equities_strategy_ids": ["aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"]},
        descriptor=descriptor,
    ) == ["aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"]
    assert parse_required_strategy_ids(
        {"equities_required_strategy_ids": ["aapl_tradexyz_makerv4"]},
        descriptor=descriptor,
        fallback=["aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"],
    ) == ["aapl_tradexyz_makerv4"]


def test_equities_live_config_prunes_shared_portfolio_contracts_to_core_prod_basket() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")
    allowlist = _equities_strategy_ids(config["api"])
    required = _required_strategy_ids(config["api"], fallback=allowlist)
    strategy_ids_by_asset = _strategy_ids_by_asset(config, allowlist=allowlist)

    assert required == allowlist
    assert _portfolio_base_assets(config) == list(strategy_ids_by_asset)
    assert set(allowlist) == {
        strategy_id
        for strategy_ids in strategy_ids_by_asset.values()
        for strategy_id in strategy_ids
    }
    assert all("_makerv4" not in strategy_id for strategy_id in allowlist)
    for asset, strategy_ids in strategy_ids_by_asset.items():
        assert len(strategy_ids) in {2, 4}
        assert {strategy_id.rsplit("_", maxsplit=1)[-1] for strategy_id in strategy_ids} == {
            "maker",
            "taker",
        }
        assert all(strategy_id.startswith(f"{asset.lower()}_") for strategy_id in strategy_ids)
        enrolled_venues = {
            strategy_id.removeprefix(f"{asset.lower()}_").rsplit("_", maxsplit=1)[0]
            for strategy_id in strategy_ids
        }
        assert len(strategy_ids) == 2 * len(enrolled_venues)


def test_portfolio_base_assets_dedupes_contract_bases() -> None:
    assert _portfolio_base_assets(
        {
            "contracts": [
                {"exchange": "hyperliquid", "symbol": "AAPL/USD"},
                {"exchange": "hyperliquid", "symbol": "AAPL/USD"},
                {"exchange": "hyperliquid", "symbol": "MSFT/USD"},
            ],
        },
    ) == ["AAPL", "MSFT"]


def test_strategy_ids_by_asset_groups_allowlisted_strategy_contracts() -> None:
    assert _strategy_ids_by_asset(
        {
            "strategy_contracts": [
                _strategy_contract(
                    "aapl_tradexyz_makerv4",
                    reference_account_scope_id="ibkr.reference.main",
                ),
                _strategy_contract(
                    "aapl_tradexyz_makerv4",
                    reference_account_scope_id="ibkr.reference.main",
                ),
                _strategy_contract(
                    "msft_tradexyz_makerv4",
                    reference_account_scope_id="ibkr.reference.main",
                ),
            ],
        },
        allowlist=["aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"],
    ) == {
        "AAPL": ("aapl_tradexyz_makerv4",),
        "MSFT": ("msft_tradexyz_makerv4",),
    }


def test_strategy_ids_by_asset_groups_distinct_same_asset_variants() -> None:
    assert _strategy_ids_by_asset(
        {
            "strategy_contracts": [
                {
                    **_strategy_contract(
                        "aapl_tradexyz_maker",
                        reference_account_scope_id="ibkr.reference.main",
                    ),
                    "portfolio_asset_id": "AAPL",
                },
                {
                    **_strategy_contract(
                        "aapl_tradexyz_taker",
                        reference_account_scope_id="ibkr.reference.main",
                    ),
                    "portfolio_asset_id": "AAPL",
                },
                {
                    **_strategy_contract(
                        "aapl_tradexyz_taker",
                        reference_account_scope_id="ibkr.reference.main",
                    ),
                    "portfolio_asset_id": "AAPL",
                },
            ],
        },
        allowlist=["aapl_tradexyz_maker", "aapl_tradexyz_taker"],
    ) == {
        "AAPL": ("aapl_tradexyz_maker", "aapl_tradexyz_taker"),
    }


def test_portfolio_aggregator_sums_allowlisted_component_keys() -> None:
    now_ms_value = int(time.time() * 1000)
    fake_redis = _FakeRedis(
        {
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="aapl_tradexyz_makerv4",
                portfolio_id="equities",
                base_currency="AAPL",
            ): encode_component(
                StrategyInventoryComponent(
                    strategy_id="aapl_tradexyz_makerv4",
                    portfolio_id="equities",
                    base_currency="AAPL",
                    local_qty_base=15,
                    ts_ms=now_ms_value,
                    state="running",
                ),
            ).encode(),
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="msft_tradexyz_makerv4",
                portfolio_id="equities",
                base_currency="AAPL",
            ): encode_component(
                StrategyInventoryComponent(
                    strategy_id="msft_tradexyz_makerv4",
                    portfolio_id="equities",
                    base_currency="AAPL",
                    local_qty_base=-5,
                    ts_ms=now_ms_value,
                    state="running",
                ),
            ).encode(),
        },
    )
    aggregator = EquitiesPortfolioAggregator.__new__(EquitiesPortfolioAggregator)
    aggregator._namespace = "flux"
    aggregator._schema_version = "v1"
    aggregator._mode = "live"
    aggregator._portfolio_id = "equities"
    aggregator._stale_after_ms = 3_000
    aggregator._strategy_ids = ["aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"]
    aggregator._required_strategy_ids = set(aggregator._strategy_ids)
    aggregator._base_assets = ["AAPL"]
    aggregator._redis = fake_redis
    aggregator._log = None

    aggregator.recompute_once()

    payload = decode_portfolio_inventory(
        fake_redis.get(
            FluxRedisKeys.portfolio_inventory(portfolio_id="equities", base_currency="AAPL"),
        ),
    )

    assert payload is not None
    assert payload["global_qty"] == "10.000000"
    assert payload["missing_required"] == []
    assert fake_redis.published


def test_build_profile_account_provider_bindings_uses_shared_account_scopes(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured_provider_configs: list[object] = []

    def _fake_cached_ibkr_provider(provider_config):
        captured_provider_configs.append(provider_config)
        return _CountingAccountProjectionProvider(rows=[])

    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.get_cached_ibkr_reference_balance_provider",
        _fake_cached_ibkr_provider,
    )

    bindings = build_profile_account_provider_bindings(
        config={
            "account_scopes": _account_scopes(),
            "strategy_contracts": [
                _strategy_contract(
                    "aapl_tradexyz_makerv4",
                    reference_account_scope_id="ibkr.reference.main",
                ),
                _strategy_contract(
                    "msft_tradexyz_makerv4",
                    reference_account_scope_id="ibkr.reference.main",
                ),
            ],
        },
    )

    assert [binding.account_scope_id for binding in bindings] == [
        "hyperliquid.xyz.main",
        "ibkr.reference.main",
        "ibkr.hedge.main",
    ]
    reference_binding = next(
        binding for binding in bindings if binding.account_scope_id == "ibkr.reference.main"
    )
    hedge_binding = next(binding for binding in bindings if binding.account_scope_id == "ibkr.hedge.main")

    assert reference_binding.provider is not None
    assert hedge_binding.provider is not None
    assert reference_binding.source_strategy_ids == (
        "aapl_tradexyz_makerv4",
        "msft_tradexyz_makerv4",
    )
    assert len(captured_provider_configs) == 2
    assert captured_provider_configs[0].dockerized_gateway is not None
    assert captured_provider_configs[0].ibg_port == 4002
    assert captured_provider_configs[0].ibg_client_id == 7
    assert captured_provider_configs[1].dockerized_gateway is not None
    assert captured_provider_configs[1].ibg_port == 4002
    assert captured_provider_configs[1].ibg_client_id == 8


def test_build_profile_account_provider_bindings_supports_binance_futures_scope(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("EQUITIES_BINANCE_API_KEY", "binance-key")
    monkeypatch.setenv("EQUITIES_BINANCE_API_SECRET", "binance-secret")

    class _FakeBinanceProvider:
        def refresh(self) -> None:
            return None

        def snapshot(self) -> dict[str, Any]:
            return {"rows": [], "totals": {}}

    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.get_cached_ibkr_reference_balance_provider",
        lambda provider_config: _CountingAccountProjectionProvider(
            rows=[],
            totals={"ibg_client_id": provider_config.ibg_client_id},
        ),
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts._build_binance_futures_account_provider",
        lambda **_kwargs: _FakeBinanceProvider(),
        raising=False,
    )

    bindings = build_profile_account_provider_bindings(
        config={
            "account_scopes": [
                {
                    "scope_id": "binance.futures.main",
                    "provider": "binance",
                    "venue": "BINANCE_PERP",
                    "api_key_env": "EQUITIES_BINANCE_API_KEY",
                    "api_secret_env": "EQUITIES_BINANCE_API_SECRET",
                    "account_type": "USDT_FUTURES",
                },
                {
                    "scope_id": "ibkr.reference.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4002,
                    "ibg_client_id": 7,
                },
                {
                    "scope_id": "ibkr.hedge.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4002,
                    "ibg_client_id": 8,
                },
            ],
            "strategy_contracts": [
                {
                    "strategy_id": "pltr_binance_perp_maker",
                    "portfolio_asset_id": "PLTR",
                    "maker_venue": "BINANCE_PERP",
                    "maker_symbol": "PLTRUSDT",
                    "market_type": "perp",
                    "maker_instrument_id": "PLTRUSDT-PERP.BINANCE_PERP",
                    "reference_instrument_id": "PLTR.NASDAQ",
                    "execution_account_scope_id": "binance.futures.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                    "hedge_account_scope_id": "ibkr.hedge.main",
                },
                {
                    "strategy_id": "pltr_binance_perp_taker",
                    "portfolio_asset_id": "PLTR",
                    "maker_venue": "BINANCE_PERP",
                    "maker_symbol": "PLTRUSDT",
                    "market_type": "perp",
                    "maker_instrument_id": "PLTRUSDT-PERP.BINANCE_PERP",
                    "reference_instrument_id": "PLTR.NASDAQ",
                    "execution_account_scope_id": "binance.futures.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                    "hedge_account_scope_id": "ibkr.hedge.main",
                },
            ],
        },
    )

    assert [binding.account_scope_id for binding in bindings] == [
        "binance.futures.main",
        "ibkr.reference.main",
        "ibkr.hedge.main",
    ]
    binance_binding = next(
        binding for binding in bindings if binding.account_scope_id == "binance.futures.main"
    )
    reference_binding = next(
        binding for binding in bindings if binding.account_scope_id == "ibkr.reference.main"
    )
    hedge_binding = next(
        binding for binding in bindings if binding.account_scope_id == "ibkr.hedge.main"
    )

    assert binance_binding.source_strategy_ids == (
        "pltr_binance_perp_maker",
        "pltr_binance_perp_taker",
    )
    assert binance_binding.provider is not None
    assert reference_binding.provider is not None
    assert hedge_binding.provider is not None


def test_build_profile_account_provider_bindings_preserves_binance_private_api_family(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from flux.common.account_scopes import AccountScopeConfig
    from flux.runners.shared import profile_accounts as profile_accounts_mod
    from nautilus_trader.adapters.binance.common.enums import BinancePrivateApiFamily

    monkeypatch.setenv("EQUITIES_BINANCE_API_KEY", "binance-key")
    monkeypatch.setenv("EQUITIES_BINANCE_API_SECRET", "binance-secret")

    captured: dict[str, Any] = {}

    class _FakeBinanceProvider:
        def __init__(self, config) -> None:
            captured["config"] = config

    monkeypatch.setattr(
        profile_accounts_mod,
        "BinanceFuturesAccountProjectionProvider",
        _FakeBinanceProvider,
    )

    provider = profile_accounts_mod._build_binance_futures_account_provider(
        scope_config=AccountScopeConfig(
            scope_id="binance.futures.main",
            provider="binance",
            venue="BINANCE_PERP",
            api_key_env="EQUITIES_BINANCE_API_KEY",
            api_secret_env="EQUITIES_BINANCE_API_SECRET",
            account_type="USDT_FUTURES",
            private_api_family="PORTFOLIO_MARGIN",
        ),
        account_scope_id="binance.futures.main",
        source_strategy_ids=("pltr_binance_perp_maker", "pltr_binance_perp_taker"),
    )

    assert provider is not None
    assert captured["config"].private_api_family == BinancePrivateApiFamily.PORTFOLIO_MARGIN


def test_build_profile_account_provider_bindings_routes_binance_portfolio_margin_to_private_base_url(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from flux.common.account_scopes import AccountScopeConfig
    from flux.runners.shared import profile_accounts as profile_accounts_mod
    from nautilus_trader.adapters.binance.common.enums import BinancePrivateApiFamily

    monkeypatch.setenv("EQUITIES_BINANCE_API_KEY", "binance-key")
    monkeypatch.setenv("EQUITIES_BINANCE_API_SECRET", "binance-secret")

    captured: dict[str, Any] = {}

    class _FakeClient:
        pass

    def _fake_cached_binance_http_client(**kwargs):
        captured["base_url"] = kwargs["base_url"]
        captured["environment"] = kwargs["environment"]
        return _FakeClient()

    class _FakeAccountHttpAPI:
        def __init__(self, **kwargs) -> None:
            captured["private_api_family"] = kwargs["private_api_family"]

    monkeypatch.setattr(
        profile_accounts_mod,
        "get_cached_binance_http_client",
        _fake_cached_binance_http_client,
    )
    monkeypatch.setattr(
        profile_accounts_mod,
        "BinanceFuturesAccountHttpAPI",
        _FakeAccountHttpAPI,
    )

    provider = profile_accounts_mod._build_binance_futures_account_provider(
        scope_config=AccountScopeConfig(
            scope_id="binance.futures.main",
            provider="binance",
            venue="BINANCE_PERP",
            api_key_env="EQUITIES_BINANCE_API_KEY",
            api_secret_env="EQUITIES_BINANCE_API_SECRET",
            account_type="USDT_FUTURES",
            private_api_family="PORTFOLIO_MARGIN",
        ),
        account_scope_id="binance.futures.main",
        source_strategy_ids=("pltr_binance_perp_maker", "pltr_binance_perp_taker"),
    )

    assert provider is not None
    assert captured["base_url"] == "https://papi.binance.com"
    assert captured["private_api_family"] == BinancePrivateApiFamily.PORTFOLIO_MARGIN


def test_build_profile_account_provider_bindings_preserves_explicit_zero_ibkr_client_id(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    captured_provider_configs: list[object] = []

    def _fake_cached_ibkr_provider(provider_config):
        captured_provider_configs.append(provider_config)
        return _CountingAccountProjectionProvider(rows=[])

    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.get_cached_ibkr_reference_balance_provider",
        _fake_cached_ibkr_provider,
    )

    build_profile_account_provider_bindings(
        config={
            "account_scopes": [
                {
                    "scope_id": "hyperliquid.xyz.main",
                    "provider": "hyperliquid",
                    "venue": "HYPERLIQUID",
                },
                {
                    "scope_id": "ibkr.reference.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4002,
                    "ibg_client_id": 0,
                },
                {
                    "scope_id": "ibkr.hedge.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4002,
                    "ibg_client_id": 8,
                },
            ],
            "strategy_contracts": [
                _strategy_contract(
                    "aapl_tradexyz_makerv4",
                    reference_account_scope_id="ibkr.reference.main",
                ),
            ],
        },
    )

    assert len(captured_provider_configs) == 2
    assert captured_provider_configs[0].ibg_client_id == 0


def test_build_profile_account_provider_bindings_rejects_missing_shared_scope() -> None:
    with pytest.raises(ValueError, match=r"ibkr\.reference\.main"):
        build_profile_account_provider_bindings(
            config={
                "account_scopes": [
                    {
                        "scope_id": "hyperliquid.xyz.main",
                        "provider": "hyperliquid",
                        "venue": "HYPERLIQUID",
                    },
                ],
                "strategy_contracts": [
                    _strategy_contract(
                        "aapl_tradexyz_makerv4",
                        reference_account_scope_id="ibkr.reference.main",
                    ),
                ],
            },
        )


def test_equities_portfolio_runner_collects_shared_account_snapshots_once_per_scope(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        "flux.runners.shared.portfolio_runner.build_redis_client",
        lambda _cfg: _FakeRedis(),
    )
    captured_provider_configs: list[object] = []

    def _fake_cached_ibkr_provider(provider_config):
        captured_provider_configs.append(provider_config)
        return _CountingAccountProjectionProvider(rows=[])

    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.get_cached_ibkr_reference_balance_provider",
        _fake_cached_ibkr_provider,
    )
    config: dict[str, Any] = {
        "flux": {"namespace": "flux", "schema_version": "v1"},
        "redis": {},
        "venues": {"reference_venue": "IBKR"},
        "account_scopes": _account_scopes(),
        "api": {
            "equities_strategy_ids": ["aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"],
        },
        "portfolio": {"portfolio_id": "equities"},
        "contracts": [{"exchange": "hyperliquid", "symbol": "AAPL/USD"}],
        "strategy_contracts": [
            _strategy_contract(
                "aapl_tradexyz_makerv4",
                reference_account_scope_id="ibkr.reference.main",
            ),
            _strategy_contract(
                "msft_tradexyz_makerv4",
                reference_account_scope_id="ibkr.reference.main",
            ),
        ],
    }

    aggregator = EquitiesPortfolioAggregator(
        config=config,
        mode="paper",
        logger=MagicMock(),
    )

    assert aggregator.account_scope_ids == [
        "hyperliquid.xyz.main",
        "ibkr.reference.main",
        "ibkr.hedge.main",
    ]
    assert len(captured_provider_configs) == 2
    assert aggregator._profile_account_bindings[1].provider is not None
    assert aggregator._profile_account_bindings[2].provider is not None


def test_equities_portfolio_runner_builds_hyperliquid_shared_account_provider(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    funded_account = "0x1111111111111111111111111111111111111111"
    vault_account = "0x2222222222222222222222222222222222222222"

    monkeypatch.setattr(
        "flux.runners.shared.portfolio_runner.build_redis_client",
        lambda _cfg: _FakeRedis(),
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.get_cached_ibkr_reference_balance_provider",
        lambda _provider_config: _CountingAccountProjectionProvider(rows=[]),
    )
    monkeypatch.setenv("TRADE_XYZ_AGENT_PK", "super-secret")
    monkeypatch.setenv("TRADE_XYZ_ACCOUNT_ADDRESS", funded_account)
    monkeypatch.setenv("TRADE_XYZ_VAULT_ADDRESS", vault_account)

    captured_client_kwargs: list[dict[str, Any]] = []
    captured_account_ids: list[str] = []
    captured_account_addresses: list[str] = []
    captured_info_payloads: list[dict[str, Any]] = []

    class _FakeHyperliquidAccountState:
        def to_dict(self) -> dict[str, Any]:
            return {
                "account_id": "HYPERLIQUID-master",
                "balances": [],
            }

    class _FakeHyperliquidClient:
        def get_user_address(self) -> str:
            return "0x3333333333333333333333333333333333333333"

        def set_account_id(self, account_id: str) -> None:
            captured_account_ids.append(account_id)

        def set_account_address(self, account_address: str) -> None:
            captured_account_addresses.append(account_address)

        async def request_account_state(self, **kwargs: Any) -> _FakeHyperliquidAccountState:
            assert kwargs == {
                "account_address": vault_account,
                "dex": "xyz",
            }
            return _FakeHyperliquidAccountState()

        async def request_position_status_reports(self, **kwargs: Any) -> list[Any]:
            assert kwargs == {
                "account_address": vault_account,
                "dex": "xyz",
            }
            return [
                nautilus_pyo3.PositionStatusReport(
                    account_id=nautilus_pyo3.AccountId("HYPERLIQUID-master"),
                    instrument_id=nautilus_pyo3.InstrumentId.from_str(
                        "xyz:NVDA-USD-PERP.HYPERLIQUID",
                    ),
                    position_side=nautilus_pyo3.PositionSide.SHORT,
                    quantity=nautilus_pyo3.Quantity.from_str("9.111"),
                    avg_px_open=183.22,
                    ts_last=1_700_000_000_010,
                    ts_init=1_700_000_000_010,
                    report_id=nautilus_pyo3.UUID4(),
                ),
                nautilus_pyo3.PositionStatusReport(
                    account_id=nautilus_pyo3.AccountId("HYPERLIQUID-master"),
                    instrument_id=nautilus_pyo3.InstrumentId.from_str(
                        "xyz:COIN-USD-PERP.HYPERLIQUID",
                    ),
                    position_side=nautilus_pyo3.PositionSide.SHORT,
                    quantity=nautilus_pyo3.Quantity.from_str("22.715"),
                    avg_px_open=194.5,
                    ts_last=1_700_000_000_020,
                    ts_init=1_700_000_000_020,
                    report_id=nautilus_pyo3.UUID4(),
                ),
                nautilus_pyo3.PositionStatusReport(
                    account_id=nautilus_pyo3.AccountId("HYPERLIQUID-master"),
                    instrument_id=nautilus_pyo3.InstrumentId.from_str(
                        "xyz:GOOGL-USD-PERP.HYPERLIQUID",
                    ),
                    position_side=nautilus_pyo3.PositionSide.SHORT,
                    quantity=nautilus_pyo3.Quantity.from_str("6"),
                    avg_px_open=303.15,
                    ts_last=1_700_000_000_000,
                    ts_init=1_700_000_000_000,
                    report_id=nautilus_pyo3.UUID4(),
                ),
            ]

    def _fake_cached_hyperliquid_client(**kwargs: Any) -> _FakeHyperliquidClient:
        captured_client_kwargs.append(dict(kwargs))
        return _FakeHyperliquidClient()

    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.get_cached_hyperliquid_http_client",
        _fake_cached_hyperliquid_client,
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.resolve_hyperliquid_user",
        lambda **_kwargs: ResolvedHyperliquidUser(
            execution_signer="0x3333333333333333333333333333333333333333",
            account_query_address=vault_account,
            fee_query_address=vault_account,
            ws_subscription_address=vault_account,
            source="vault_address",
        ),
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts._post_hyperliquid_info",
        lambda **kwargs: (
            captured_info_payloads.append(dict(kwargs["payload"]))
            or (
                {
                    "marginSummary": {
                        "accountValue": "7478.386872",
                    },
                    "withdrawable": "7478.386872",
                }
                if kwargs["payload"]["type"] == "clearinghouseState"
                else {
                    "balances": [
                        {
                            "coin": "USDC",
                            "total": "0.002096",
                            "hold": "0.0",
                        },
                        {
                            "coin": "USDE",
                            "total": "1075.27105006",
                            "hold": "0.0",
                        },
                    ],
                }
            )
        ),
    )
    config: dict[str, Any] = {
        "flux": {"namespace": "flux", "schema_version": "v1"},
        "redis": {},
        "venues": {"reference_venue": "IBKR"},
        "account_scopes": _account_scopes(),
        "api": {
            "equities_strategy_ids": ["aapl_tradexyz_makerv4"],
        },
        "portfolio": {"portfolio_id": "equities"},
        "contracts": [{"exchange": "hyperliquid", "symbol": "AAPL/USD"}],
        "strategy_contracts": [
            _strategy_contract(
                "aapl_tradexyz_makerv4",
                reference_account_scope_id="ibkr.reference.main",
            ),
        ],
    }

    aggregator = EquitiesPortfolioAggregator(
        config=config,
        mode="paper",
        logger=MagicMock(),
    )

    assert aggregator._profile_account_bindings[0].account_scope_id == "hyperliquid.xyz.main"
    assert aggregator._profile_account_bindings[0].provider is not None
    aggregator._profile_account_bindings[0].provider.refresh()
    snapshot = aggregator._profile_account_bindings[0].provider.snapshot()
    assert snapshot is not None
    assert captured_client_kwargs == [
        {
            "private_key": "super-secret",
            "account_address": funded_account,
            "vault_address": vault_account,
            "timeout_secs": 10,
            "testnet": False,
            "proxy_url": None,
            "dex": "xyz",
        },
    ]
    assert captured_account_ids == ["HYPERLIQUID-master"]
    assert captured_account_addresses == [vault_account]
    assert captured_info_payloads == [
        {"type": "clearinghouseState", "user": vault_account, "dex": "xyz"},
        {"type": "spotClearinghouseState", "user": vault_account, "dex": "xyz"},
    ]
    assert {
        (row["exchange"], row["asset"], row.get("kind"), row.get("contract_type"))
        for row in snapshot["rows"]
    } >= {
        ("hyperliquid", "USDC", None, "cash"),
        ("hyperliquid", "USDE", None, "cash"),
        ("hyperliquid", "NVDA", "position", "perp"),
        ("hyperliquid", "COIN", "position", "perp"),
        ("hyperliquid", "GOOGL", "position", "perp"),
    }
    hyperliquid_position_rows = [
        row
        for row in snapshot["rows"]
        if row["exchange"] == "hyperliquid" and row.get("kind") == "position"
    ]
    assert {row["instrument_id"] for row in hyperliquid_position_rows} >= {
        "XYZ:NVDA-USD-PERP.HYPERLIQUID",
        "XYZ:COIN-USD-PERP.HYPERLIQUID",
        "XYZ:GOOGL-USD-PERP.HYPERLIQUID",
    }
    assert snapshot["totals"]["account_equity_raw"] == pytest.approx(7478.386872)
    assert snapshot["totals"]["withdrawable_raw"] == pytest.approx(7478.386872)



def test_equities_portfolio_aggregator_publishes_account_projection_once_per_scope() -> None:
    provider = _CountingAccountProjectionProvider(
        rows=[
            {
                "exchange": "ibkr",
                "account": "U1234567",
                "asset": "AAPL",
                "kind": "position",
                "signed_qty": "25",
            },
        ],
    )
    fake_redis = _FakeRedis()
    aggregator = EquitiesPortfolioAggregator.__new__(EquitiesPortfolioAggregator)
    aggregator._descriptor = get_strategy_set_descriptor("equities")
    aggregator._namespace = "flux"
    aggregator._schema_version = "v1"
    aggregator._mode = "live"
    aggregator._portfolio_id = "equities"
    aggregator._stale_after_ms = 3_000
    aggregator._strategy_ids = []
    aggregator._required_strategy_ids = set()
    aggregator._base_assets = []
    aggregator._redis = fake_redis
    aggregator._log = MagicMock()
    aggregator.account_scope_ids = ["ibkr.reference.main"]
    aggregator._profile_account_bindings = (
        ProfileAccountProviderBinding(
            account_scope_id="ibkr.reference.main",
            source_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
            provider=provider,
        ),
    )

    aggregator.recompute_once()

    raw_snapshot = fake_redis.get(
        FluxRedisKeys.profile_account_projection(
            profile_id="equities",
            account_scope_id="ibkr.reference.main",
        ),
    )
    assert raw_snapshot is not None
    snapshot = json.loads(raw_snapshot)
    assert snapshot["rows"][0]["source_scope"] == "shared_account"
    assert snapshot["rows"][0]["account_scope_id"] == "ibkr.reference.main"
    assert provider.refresh_calls == 1
    assert (
        FluxRedisKeys.profile_account_projection_channel(
            profile_id="equities",
            account_scope_id="ibkr.reference.main",
        ),
        raw_snapshot.decode(),
    ) in fake_redis.published


def test_equities_portfolio_aggregator_publishes_multi_asset_portfolio_snapshot_v2() -> None:
    now_ms_value = int(time.time() * 1000)
    provider = _CountingAccountProjectionProvider(
        rows=[
            {
                "exchange": "ibkr",
                "account": "U1234567",
                "asset": "AAPL",
                "kind": "position",
                "signed_qty": "25",
                "account_scope_id": "ibkr.reference.main",
                "source_scope": "shared_account",
            },
        ],
    )
    fake_redis = _FakeRedis(
        {
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="aapl_tradexyz_makerv4",
                portfolio_id="equities",
                base_currency="AAPL",
            ): encode_component(
                StrategyInventoryComponent(
                    strategy_id="aapl_tradexyz_makerv4",
                    portfolio_id="equities",
                    base_currency="AAPL",
                    local_qty_base=10,
                    ts_ms=now_ms_value,
                    state="running",
                ),
            ).encode(),
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="msft_tradexyz_makerv4",
                portfolio_id="equities",
                base_currency="MSFT",
            ): encode_component(
                StrategyInventoryComponent(
                    strategy_id="msft_tradexyz_makerv4",
                    portfolio_id="equities",
                    base_currency="MSFT",
                    local_qty_base=5,
                    ts_ms=now_ms_value,
                    state="running",
                ),
            ).encode(),
            FluxRedisKeys(strategy_id="aapl_tradexyz_makerv4").balances_snapshot(): json.dumps(
                [
                    {
                        "strategy_id": "aapl_tradexyz_makerv4",
                        "exchange": "hyperliquid",
                        "asset": "USD",
                        "account": "trading",
                        "total": "100",
                        "ts_ms": now_ms_value,
                    },
                ],
            ).encode(),
            FluxRedisKeys(strategy_id="msft_tradexyz_makerv4").balances_snapshot(): json.dumps(
                [
                    {
                        "strategy_id": "msft_tradexyz_makerv4",
                        "exchange": "hyperliquid",
                        "asset": "USD",
                        "account": "trading",
                        "total": "50",
                        "ts_ms": now_ms_value,
                    },
                ],
            ).encode(),
        },
    )
    aggregator = EquitiesPortfolioAggregator.__new__(EquitiesPortfolioAggregator)
    aggregator._descriptor = get_strategy_set_descriptor("equities")
    aggregator._namespace = "flux"
    aggregator._schema_version = "v1"
    aggregator._mode = "live"
    aggregator._portfolio_id = "equities"
    aggregator._stale_after_ms = 3_000
    aggregator._aggregation_mode = "strict"
    aggregator._strategy_ids = ["aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"]
    aggregator._required_strategy_ids = set(aggregator._strategy_ids)
    aggregator._base_assets = ["AAPL", "MSFT"]
    aggregator._strategy_ids_by_asset = {
        "AAPL": ("aapl_tradexyz_makerv4",),
        "MSFT": ("msft_tradexyz_makerv4",),
    }
    aggregator._redis = fake_redis
    aggregator._log = MagicMock()
    aggregator.account_scope_ids = ["ibkr.reference.main"]
    aggregator._profile_account_bindings = (
        ProfileAccountProviderBinding(
            account_scope_id="ibkr.reference.main",
            source_strategy_ids=("aapl_tradexyz_makerv4", "msft_tradexyz_makerv4"),
            provider=provider,
        ),
    )

    aggregator.recompute_once()

    raw_snapshot = fake_redis.get(FluxRedisKeys.portfolio_snapshot(portfolio_id="equities"))
    assert raw_snapshot is not None
    snapshot = json.loads(raw_snapshot)

    assert sorted(snapshot["inventory_by_asset"]) == ["AAPL", "MSFT"]
    assert snapshot["inventory_by_asset"]["AAPL"]["global_qty_base"] == "10.000000"
    assert snapshot["inventory_by_asset"]["MSFT"]["global_qty_base"] == "5.000000"
    assert "base_currency" not in snapshot
    assert "inventory" not in snapshot
    assert snapshot["accounts"]["rows"][0]["account_scope_id"] == "ibkr.reference.main"
    assert snapshot["balances"]["rows"][0]["strategy_id"] == "equities"


def test_equities_portfolio_aggregator_deduplicates_shared_same_asset_observations() -> None:
    now_ms_value = int(time.time() * 1000)
    fake_redis = _FakeRedis(
        {
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="aapl_tradexyz_maker",
                portfolio_id="equities",
                base_currency="AAPL",
            ): encode_component(
                StrategyInventoryComponent(
                    strategy_id="aapl_tradexyz_maker",
                    portfolio_id="equities",
                    base_currency="AAPL",
                    local_qty_base=10,
                    ts_ms=now_ms_value,
                    maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
                    state="running",
                ),
            ).encode(),
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="aapl_tradexyz_taker",
                portfolio_id="equities",
                base_currency="AAPL",
            ): encode_component(
                StrategyInventoryComponent(
                    strategy_id="aapl_tradexyz_taker",
                    portfolio_id="equities",
                    base_currency="AAPL",
                    local_qty_base=None,
                    ts_ms=now_ms_value,
                    maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
                    state="running",
                ),
            ).encode(),
            FluxRedisKeys(strategy_id="aapl_tradexyz_maker").balances_snapshot(): json.dumps(
                [
                    {
                        "strategy_id": "aapl_tradexyz_maker",
                        "exchange": "hyperliquid",
                        "account": "trading",
                        "asset": "AAPL",
                        "kind": "position",
                        "instrument_id": "XYZ:AAPL-USD-PERP.HYPERLIQUID",
                        "signed_qty": "10",
                        "quantity": "10",
                        "ts_ms": now_ms_value,
                    },
                ],
            ).encode(),
            FluxRedisKeys(strategy_id="aapl_tradexyz_taker").balances_snapshot(): json.dumps(
                [
                    {
                        "strategy_id": "aapl_tradexyz_taker",
                        "exchange": "hyperliquid",
                        "account": "trading",
                        "asset": "AAPL",
                        "kind": "position",
                        "instrument_id": "XYZ:AAPL-USD-PERP.HYPERLIQUID",
                        "signed_qty": "10",
                        "quantity": "10",
                        "ts_ms": now_ms_value,
                    },
                ],
            ).encode(),
        },
    )
    aggregator = EquitiesPortfolioAggregator.__new__(EquitiesPortfolioAggregator)
    aggregator._descriptor = get_strategy_set_descriptor("equities")
    aggregator._namespace = "flux"
    aggregator._schema_version = "v1"
    aggregator._mode = "live"
    aggregator._portfolio_id = "equities"
    aggregator._stale_after_ms = 3_000
    aggregator._aggregation_mode = "strict"
    aggregator._strategy_ids = ["aapl_tradexyz_maker", "aapl_tradexyz_taker"]
    aggregator._required_strategy_ids = set(aggregator._strategy_ids)
    aggregator._base_assets = ["AAPL"]
    aggregator._strategy_ids_by_asset = {
        "AAPL": ("aapl_tradexyz_maker", "aapl_tradexyz_taker"),
    }
    aggregator._shared_observation_group_by_strategy_id = {
        "aapl_tradexyz_maker": "AAPL|hyperliquid.xyz.main|xyz:AAPL-USD-PERP.HYPERLIQUID",
        "aapl_tradexyz_taker": "AAPL|hyperliquid.xyz.main|xyz:AAPL-USD-PERP.HYPERLIQUID",
    }
    aggregator._redis = fake_redis
    aggregator._log = MagicMock()
    aggregator.account_scope_ids = []
    aggregator._profile_account_bindings = ()

    aggregator.recompute_once()

    raw_snapshot = fake_redis.get(FluxRedisKeys.portfolio_snapshot(portfolio_id="equities"))
    assert raw_snapshot is not None
    snapshot = json.loads(raw_snapshot)

    assert snapshot["inventory_by_asset"]["AAPL"]["global_qty_base"] == "10.000000"
    assert snapshot["inventory_by_asset"]["AAPL"]["degraded"] is False
    component_rows = snapshot["inventory_by_asset"]["AAPL"]["components"]
    assert [row["strategy_id"] for row in component_rows] == [
        "aapl_tradexyz_maker",
        "aapl_tradexyz_taker",
    ]
    assert [row["local_qty_base"] for row in component_rows] == ["10.000000", None]
    position_rows = [
        row
        for row in snapshot["balances"]["rows"]
        if row["exchange"] == "hyperliquid" and row.get("kind") == "position"
    ]
    assert len(position_rows) == 1
    assert position_rows[0]["strategy_id"] == "equities"
    assert position_rows[0]["signed_qty"] == "10"


def test_equities_portfolio_aggregator_publishes_shared_hyperliquid_cash_positions_and_totals() -> None:
    now_ms_value = int(time.time() * 1000)
    provider = _CountingAccountProjectionProvider(
        rows=[
            {
                "exchange": "hyperliquid",
                "account": "HYPERLIQUID-master",
                "asset": "USDE",
                "total": "1075.37415731",
                "account_scope_id": "hyperliquid.xyz.main",
                "source_scope": "shared_account",
            },
            {
                "exchange": "hyperliquid",
                "account": "HYPERLIQUID-master",
                "asset": "NVDA",
                "kind": "position",
                "instrument_id": "XYZ:NVDA-USD-PERP.HYPERLIQUID",
                "signed_qty": "-9.111",
                "quantity": "9.111",
                "account_scope_id": "hyperliquid.xyz.main",
                "source_scope": "shared_account",
            },
        ],
        totals={
            "account_equity_raw": 8314.466609,
            "withdrawable_raw": 0.0,
        },
    )
    fake_redis = _FakeRedis()
    aggregator = EquitiesPortfolioAggregator.__new__(EquitiesPortfolioAggregator)
    aggregator._descriptor = get_strategy_set_descriptor("equities")
    aggregator._namespace = "flux"
    aggregator._schema_version = "v1"
    aggregator._mode = "live"
    aggregator._portfolio_id = "equities"
    aggregator._stale_after_ms = 3_000
    aggregator._aggregation_mode = "strict"
    aggregator._strategy_ids = ["aapl_tradexyz_makerv4"]
    aggregator._required_strategy_ids = set(aggregator._strategy_ids)
    aggregator._base_assets = ["AAPL"]
    aggregator._strategy_ids_by_asset = {
        "AAPL": ("aapl_tradexyz_makerv4",),
    }
    aggregator._redis = fake_redis
    aggregator._log = MagicMock()
    aggregator.account_scope_ids = ["hyperliquid.xyz.main"]
    aggregator._profile_account_bindings = (
        ProfileAccountProviderBinding(
            account_scope_id="hyperliquid.xyz.main",
            source_strategy_ids=("aapl_tradexyz_makerv4",),
            provider=provider,
        ),
    )

    aggregator.recompute_once()

    raw_snapshot = fake_redis.get(FluxRedisKeys.portfolio_snapshot(portfolio_id="equities"))
    assert raw_snapshot is not None
    snapshot = json.loads(raw_snapshot)

    hyperliquid_rows = [
        row for row in snapshot["accounts"]["rows"] if row["exchange"] == "hyperliquid"
    ]
    assert {row["asset"] for row in hyperliquid_rows} >= {"USDE", "NVDA"}
    assert {row.get("kind") for row in hyperliquid_rows if row["asset"] == "NVDA"} == {"position"}
    assert snapshot["accounts"]["totals"]["account_equity_raw"] == pytest.approx(8314.466609)
    assert snapshot["accounts"]["totals"]["withdrawable_raw"] == pytest.approx(0.0)


def test_equities_portfolio_aggregator_run_closes_redis_on_exit_with_legacy_disconnect(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    aggregator = EquitiesPortfolioAggregator.__new__(EquitiesPortfolioAggregator)
    aggregator._descriptor = get_strategy_set_descriptor("equities")
    aggregator._portfolio_id = "equities"
    aggregator._mode = "paper"
    aggregator._base_assets = ["AAPL"]
    aggregator._strategy_ids = ["aapl_tradexyz_makerv4"]
    aggregator._redis = _LegacyDisconnectRedis()
    aggregator._log = MagicMock()
    aggregator._running = True

    def _recompute_once() -> None:
        aggregator.stop()

    aggregator.recompute_once = _recompute_once
    monkeypatch.setattr(time, "sleep", lambda _secs: None)

    aggregator.run()

    assert aggregator._redis.closed is True
    assert aggregator._redis.connection_pool.disconnect_calls == [False]
