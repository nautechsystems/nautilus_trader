from __future__ import annotations

import json
import time
import tomllib
from pathlib import Path
from types import SimpleNamespace
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
from flux.runners.shared.profile_accounts import build_account_projection_provider
from flux.runners.shared.profile_accounts import build_profile_account_provider_bindings
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from nautilus_trader.flux.common.account_scopes import decode_account_scopes
from nautilus_trader.core import nautilus_pyo3

CORE_PROD_STRATEGY_IDS = (
    "aapl_tradexyz_makerv4",
    "amd_tradexyz_makerv4",
    "amzn_binance_perp_makerv4",
    "amzn_tradexyz_makerv4",
    "baba_tradexyz_makerv4",
    "coin_binance_perp_makerv4",
    "coin_tradexyz_makerv4",
    "crcl_binance_perp_makerv4",
    "crcl_tradexyz_makerv4",
    "crwv_tradexyz_makerv4",
    "ewy_binance_perp_makerv4",
    "googl_tradexyz_makerv4",
    "hood_binance_perp_makerv4",
    "hood_tradexyz_makerv4",
    "intc_binance_perp_makerv4",
    "intc_tradexyz_makerv4",
    "meta_tradexyz_makerv4",
    "msft_tradexyz_makerv4",
    "mstr_binance_perp_makerv4",
    "mstr_tradexyz_makerv4",
    "mu_tradexyz_makerv4",
    "nflx_tradexyz_makerv4",
    "nvda_tradexyz_makerv4",
    "orcl_tradexyz_makerv4",
    "pltr_binance_perp_makerv4",
    "pltr_tradexyz_makerv4",
    "rivn_tradexyz_makerv4",
    "sndk_tradexyz_makerv4",
    "tsla_binance_perp_makerv4",
    "tsla_tradexyz_makerv4",
    "tsm_tradexyz_makerv4",
    "usar_tradexyz_makerv4",
)
CORE_PROD_STRATEGY_IDS_BY_ASSET = {
    "AAPL": ("aapl_tradexyz_makerv4",),
    "AMD": ("amd_tradexyz_makerv4",),
    "AMZN": ("amzn_binance_perp_makerv4", "amzn_tradexyz_makerv4"),
    "BABA": ("baba_tradexyz_makerv4",),
    "COIN": ("coin_binance_perp_makerv4", "coin_tradexyz_makerv4"),
    "CRCL": ("crcl_binance_perp_makerv4", "crcl_tradexyz_makerv4"),
    "CRWV": ("crwv_tradexyz_makerv4",),
    "EWY": ("ewy_binance_perp_makerv4",),
    "GOOGL": ("googl_tradexyz_makerv4",),
    "HOOD": ("hood_binance_perp_makerv4", "hood_tradexyz_makerv4"),
    "INTC": ("intc_binance_perp_makerv4", "intc_tradexyz_makerv4"),
    "META": ("meta_tradexyz_makerv4",),
    "MSFT": ("msft_tradexyz_makerv4",),
    "MSTR": ("mstr_binance_perp_makerv4", "mstr_tradexyz_makerv4"),
    "MU": ("mu_tradexyz_makerv4",),
    "NFLX": ("nflx_tradexyz_makerv4",),
    "NVDA": ("nvda_tradexyz_makerv4",),
    "ORCL": ("orcl_tradexyz_makerv4",),
    "PLTR": ("pltr_binance_perp_makerv4", "pltr_tradexyz_makerv4"),
    "RIVN": ("rivn_tradexyz_makerv4",),
    "SNDK": ("sndk_tradexyz_makerv4",),
    "TSLA": ("tsla_binance_perp_makerv4", "tsla_tradexyz_makerv4"),
    "TSM": ("tsm_tradexyz_makerv4",),
    "USAR": ("usar_tradexyz_makerv4",),
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

    def delete(self, key: str) -> int:
        existed = key in self.values
        self.values.pop(key, None)
        return 1 if existed else 0

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


class _SequenceAccountProjectionProvider:
    def __init__(self, snapshots: list[dict[str, Any]]) -> None:
        self._snapshots = list(snapshots)
        self._index = -1
        self.refresh_calls = 0

    def refresh(self) -> None:
        self.refresh_calls += 1
        if self._index < (len(self._snapshots) - 1):
            self._index += 1

    def snapshot(self) -> dict[str, Any] | None:
        index = self._index if self._index >= 0 else 0
        return dict(self._snapshots[index])


def _strategy_contract(strategy_id: str, *, reference_account_scope_id: str) -> dict[str, str]:
    asset = strategy_id.split("_", maxsplit=1)[0].upper()
    return {
        "strategy_id": strategy_id,
        "portfolio_asset_id": asset,
        "maker_venue": "HYPERLIQUID",
        "maker_symbol": asset,
        "market_type": "perp",
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
                "manage_container": False,
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


def test_equities_live_config_enrolls_full_makerv4_portfolio_contracts() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")
    allowlist = _equities_strategy_ids(config["api"])
    required = _required_strategy_ids(config["api"], fallback=allowlist)

    assert allowlist == list(CORE_PROD_STRATEGY_IDS)
    assert required == list(CORE_PROD_STRATEGY_IDS)
    assert _portfolio_base_assets(config) == list(CORE_PROD_STRATEGY_IDS_BY_ASSET)
    assert _strategy_ids_by_asset(config, allowlist=allowlist) == CORE_PROD_STRATEGY_IDS_BY_ASSET


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


def test_strategy_ids_by_asset_groups_multiple_routes_under_one_stock_bucket() -> None:
    grouped = _strategy_ids_by_asset(
        {
            "strategy_contracts": [
                {
                    "strategy_id": "pltr_tradexyz_makerv4",
                    "portfolio_asset_id": "PLTR",
                    "maker_venue": "HYPERLIQUID",
                    "maker_symbol": "PLTR",
                    "market_type": "perp",
                    "maker_instrument_id": "xyz:PLTR-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "PLTR.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                },
                {
                    "strategy_id": "pltr_binance_perp_makerv4",
                    "portfolio_asset_id": "PLTR",
                    "maker_venue": "BINANCE_PERP",
                    "maker_symbol": "PLTRUSDT",
                    "market_type": "perp",
                    "maker_instrument_id": "PLTRUSDT-PERP.BINANCE_PERP",
                    "reference_instrument_id": "PLTR.NASDAQ",
                    "execution_account_scope_id": "binance.futures.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                },
            ],
        },
        allowlist=["pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4"],
    )

    assert grouped["PLTR"] == ("pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4")


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
    assert captured_provider_configs[0].ibg_port is None
    assert captured_provider_configs[0].ibg_client_id == 7
    assert captured_provider_configs[1].dockerized_gateway is None
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
            ],
            "strategy_contracts": [
                {
                    "strategy_id": "pltr_binance_perp_makerv4",
                    "portfolio_asset_id": "PLTR",
                    "maker_venue": "BINANCE_PERP",
                    "maker_symbol": "PLTRUSDT",
                    "market_type": "perp",
                    "maker_instrument_id": "PLTRUSDT-PERP.BINANCE_PERP",
                    "reference_instrument_id": "PLTR.NASDAQ",
                    "execution_account_scope_id": "binance.futures.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                },
            ],
        },
    )

    assert [binding.account_scope_id for binding in bindings] == [
        "binance.futures.main",
        "ibkr.reference.main",
    ]
    binance_binding = next(
        binding for binding in bindings if binding.account_scope_id == "binance.futures.main"
    )
    reference_binding = next(
        binding for binding in bindings if binding.account_scope_id == "ibkr.reference.main"
    )

    assert binance_binding.source_strategy_ids == ("pltr_binance_perp_makerv4",)
    assert binance_binding.provider is not None
    assert reference_binding.provider is not None


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


def test_equities_portfolio_runner_builds_hyperliquid_shared_account_provider_prefers_spot_usdc_total(
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

    class _FakeHyperliquidClient:
        def get_user_address(self) -> str:
            return "0x3333333333333333333333333333333333333333"

        def set_account_id(self, account_id: str) -> None:
            assert account_id == "HYPERLIQUID-master"

        def set_account_address(self, account_address: str) -> None:
            assert account_address == vault_account

        async def request_position_status_reports(self, **kwargs: Any) -> list[Any]:
            assert kwargs == {
                "account_address": vault_account,
                "dex": "xyz",
            }
            return []

    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.get_cached_hyperliquid_http_client",
        lambda **_kwargs: _FakeHyperliquidClient(),
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
            {
                "marginSummary": {
                    "accountValue": "16020",
                    "totalRawUsd": "16000",
                },
                "withdrawable": "15980",
            }
            if kwargs["payload"]["type"] == "clearinghouseState"
            else {
                "balances": [
                    {
                        "coin": "USDC",
                        "total": "16917.55422992",
                        "hold": "883.731772",
                    },
                    {
                        "coin": "USDE",
                        "total": "12.5",
                        "hold": "0.0",
                    },
                ],
            }
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

    binding = next(
        binding
        for binding in aggregator._profile_account_bindings
        if binding.account_scope_id == "hyperliquid.xyz.main"
    )
    assert binding.provider is not None
    binding.provider.refresh()
    snapshot = binding.provider.snapshot()

    assert snapshot is not None
    usdc_row = next(
        row
        for row in snapshot["rows"]
        if row["exchange"] == "hyperliquid" and row["asset"] == "USDC" and row.get("kind") != "position"
    )
    assert usdc_row["free"] == "16917.55422992"
    assert usdc_row["locked"] == "883.731772"
    assert usdc_row["total"] == "16917.55422992"
    assert usdc_row["display_name_short"] == "USDC"
    assert any(
        row["exchange"] == "hyperliquid" and row["asset"] == "USDE"
        for row in snapshot["rows"]
    )
    assert snapshot["totals"]["account_equity_raw"] == pytest.approx(16020.0)
    assert snapshot["totals"]["withdrawable_raw"] == pytest.approx(15980.0)


def test_equities_portfolio_runner_builds_binance_shared_account_provider(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        "flux.runners.shared.portfolio_runner.build_redis_client",
        lambda _cfg: _FakeRedis(),
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.get_cached_ibkr_reference_balance_provider",
        lambda _provider_config: _CountingAccountProjectionProvider(rows=[]),
    )
    monkeypatch.setenv("EQUITIES_BINANCE_API_KEY", "binance-key")
    monkeypatch.setenv("EQUITIES_BINANCE_API_SECRET", "binance-secret")

    captured_http_client_kwargs: list[dict[str, Any]] = []
    captured_account_http_inits: list[dict[str, Any]] = []
    captured_spot_account_http_inits: list[dict[str, Any]] = []

    class _FakeBinanceFuturesAccountHttpAPI:
        def __init__(
            self,
            client: Any,
            clock: Any,
            account_type: Any,
            private_api_family: Any | None = None,
        ) -> None:
            captured_account_http_inits.append(
                {
                    "client": client,
                    "clock_type": type(clock).__name__,
                    "account_type": str(getattr(account_type, "value", account_type)),
                    "private_api_family": str(
                        getattr(private_api_family, "value", private_api_family),
                    ),
                },
            )

        async def query_futures_account_info(self, recv_window: str | None = None) -> Any:
            assert recv_window == "5000"
            return SimpleNamespace(
                totalMarginBalance="1025.5",
                availableBalance="1000.0",
                maxWithdrawAmount="995.0",
                assets=[
                    SimpleNamespace(
                        asset="USDT",
                        walletBalance="1000.0",
                        availableBalance="975.0",
                    ),
                    SimpleNamespace(
                        asset="BNB",
                        walletBalance="2.5",
                        availableBalance="2.0",
                    ),
                ],
            )

        async def query_futures_position_risk(
            self,
            symbol: str | None = None,
            recv_window: str | None = None,
        ) -> list[Any]:
            assert symbol is None
            assert recv_window == "5000"
            return [
                SimpleNamespace(
                    symbol="PLTRUSDT",
                    positionAmt="12.5",
                    positionSide="BOTH",
                    entryPrice="28.5",
                    markPrice="29.0",
                    unRealizedProfit="6.25",
                    updateTime=1_700_000_000_100,
                ),
                SimpleNamespace(
                    symbol="TSLAUSDT",
                    positionAmt="0",
                    positionSide="BOTH",
                    entryPrice="0",
                    markPrice="0",
                    unRealizedProfit="0",
                    updateTime=1_700_000_000_200,
                ),
            ]

    class _FakeBinanceSpotAccountHttpAPI:
        def __init__(
            self,
            client: Any,
            clock: Any,
            account_type: Any,
        ) -> None:
            captured_spot_account_http_inits.append(
                {
                    "client": client,
                    "clock_type": type(clock).__name__,
                    "account_type": str(getattr(account_type, "value", account_type)),
                },
            )

        async def query_spot_account_info(self, recv_window: str | None = None) -> Any:
            assert recv_window == "5000"
            return SimpleNamespace(
                balances=[
                    SimpleNamespace(
                        asset="USDT",
                        free="5000.0",
                        locked="0.0",
                    ),
                ],
            )

    def _fake_cached_binance_http_client(**kwargs: Any) -> object:
        captured_http_client_kwargs.append(dict(kwargs))
        return object()

    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.get_cached_binance_http_client",
        _fake_cached_binance_http_client,
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.BinanceFuturesAccountHttpAPI",
        _FakeBinanceFuturesAccountHttpAPI,
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.BinanceSpotAccountHttpAPI",
        _FakeBinanceSpotAccountHttpAPI,
    )

    config: dict[str, Any] = {
        "flux": {"namespace": "flux", "schema_version": "v1"},
        "redis": {},
        "venues": {"reference_venue": "IBKR"},
        "account_scopes": [
            {
                "scope_id": "binance.futures.main",
                "provider": "binance",
                "venue": "BINANCE_PERP",
                "api_key_env": "EQUITIES_BINANCE_API_KEY",
                "api_secret_env": "EQUITIES_BINANCE_API_SECRET",
                "account_type": "USDT_FUTURES",
                "private_api_family": "PORTFOLIO_MARGIN",
                "recv_window_ms": 5000,
            },
            {
                "scope_id": "ibkr.reference.main",
                "provider": "ibkr",
                "venue": "IBKR",
                "ibg_host": "127.0.0.1",
                "ibg_port": 4002,
                "ibg_client_id": 7,
            },
        ],
        "api": {
            "equities_strategy_ids": ["pltr_binance_perp_makerv4"],
        },
        "portfolio": {"portfolio_id": "equities"},
        "contracts": [{"exchange": "binance_perp", "symbol": "PLTR/USDT"}],
        "strategy_contracts": [
            {
                "strategy_id": "pltr_binance_perp_makerv4",
                "portfolio_asset_id": "PLTR",
                "maker_venue": "BINANCE_PERP",
                "maker_symbol": "PLTRUSDT",
                "market_type": "perp",
                "maker_instrument_id": "PLTRUSDT-PERP.BINANCE_PERP",
                "reference_instrument_id": "PLTR.NASDAQ",
                "execution_account_scope_id": "binance.futures.main",
                "reference_account_scope_id": "ibkr.reference.main",
            },
        ],
    }

    aggregator = EquitiesPortfolioAggregator(
        config=config,
        mode="paper",
        logger=MagicMock(),
    )

    binding = next(
        binding
        for binding in aggregator._profile_account_bindings
        if binding.account_scope_id == "binance.futures.main"
    )
    assert binding.provider is not None
    binding.provider.refresh()
    snapshot = binding.provider.snapshot()

    assert snapshot is not None
    assert len(captured_http_client_kwargs) == 2
    futures_http_client_kwargs = next(
        kwargs
        for kwargs in captured_http_client_kwargs
        if str(getattr(kwargs["account_type"], "value", kwargs["account_type"])) == "USDT_FUTURES"
    )
    spot_http_client_kwargs = next(
        kwargs
        for kwargs in captured_http_client_kwargs
        if str(getattr(kwargs["account_type"], "value", kwargs["account_type"])) == "SPOT"
    )
    assert futures_http_client_kwargs["api_key"] == "binance-key"
    assert futures_http_client_kwargs["api_secret"] == "binance-secret"
    assert futures_http_client_kwargs["base_url"] == "https://papi.binance.com"
    assert spot_http_client_kwargs["base_url"] == "https://api.binance.com"
    assert captured_account_http_inits[0]["private_api_family"] == "PORTFOLIO_MARGIN"
    assert len(captured_account_http_inits) == 1
    assert captured_account_http_inits[0]["clock_type"] == "LiveClock"
    assert captured_account_http_inits[0]["account_type"] == "USDT_FUTURES"
    assert len(captured_spot_account_http_inits) == 1
    assert captured_spot_account_http_inits[0]["clock_type"] == "LiveClock"
    assert captured_spot_account_http_inits[0]["account_type"] == "SPOT"
    assert snapshot["source_scope"] == "shared_account"
    assert snapshot["totals"]["account_equity_raw"] == pytest.approx(1025.5)
    assert snapshot["totals"]["withdrawable_raw"] == pytest.approx(995.0)
    spot_cash_row = next(
        row
        for row in snapshot["rows"]
        if row.get("kind") != "position" and row["exchange"] == "binance"
    )
    perp_cash_row = next(
        row
        for row in snapshot["rows"]
        if row.get("kind") != "position" and row["exchange"] == "binance_perp"
    )
    position_row = next(row for row in snapshot["rows"] if row.get("kind") == "position")
    assert spot_cash_row["asset"] == "USDT"
    assert spot_cash_row["total"] == "5000"
    assert spot_cash_row["market_type"] == "spot"
    assert perp_cash_row["exchange"] == "binance_perp"
    assert perp_cash_row["market_type"] == "perp"
    assert position_row["instrument_id"] == "PLTRUSDT-PERP.BINANCE_PERP"
    assert position_row["asset"] == "PLTR"
    assert position_row["signed_qty"] == "12.5"
    assert position_row["market_type"] == "perp"


def test_equities_portfolio_runner_parses_portfolio_margin_balances_without_wallet_balance(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("EQUITIES_BINANCE_API_KEY", "binance-key")
    monkeypatch.setenv("EQUITIES_BINANCE_API_SECRET", "binance-secret")

    class _FakeBinanceFuturesAccountHttpAPI:
        def __init__(
            self,
            client: Any,
            clock: Any,
            account_type: Any,
            private_api_family: Any | None = None,
        ) -> None:
            _ = (client, clock, account_type, private_api_family)

        async def query_futures_account_info(self, recv_window: str | None = None) -> Any:
            assert recv_window == "5000"
            return SimpleNamespace(
                totalMarginBalance="1025.5",
                availableBalance="1000.0",
                maxWithdrawAmount="995.0",
                assets=[
                    SimpleNamespace(
                        asset="USDT",
                        marginBalance="1000.0",
                        availableBalance="975.0",
                    ),
                ],
            )

        async def query_futures_position_risk(
            self,
            symbol: str | None = None,
            recv_window: str | None = None,
        ) -> list[Any]:
            assert symbol is None
            assert recv_window == "5000"
            return []

    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.get_cached_binance_http_client",
        lambda **kwargs: object(),
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.BinanceFuturesAccountHttpAPI",
        _FakeBinanceFuturesAccountHttpAPI,
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.BinanceSpotAccountHttpAPI",
        type(
            "_FakeBinanceSpotAccountHttpAPI",
            (),
            {
                "__init__": lambda self, client, clock, account_type: None,
                "query_spot_account_info": lambda self, recv_window=None: asyncio.sleep(0, result=SimpleNamespace(balances=[])),
            },
        ),
    )

    provider = build_account_projection_provider(
        scope_config=decode_account_scopes(
            [
                {
                    "scope_id": "binance.futures.main",
                    "provider": "binance",
                    "venue": "BINANCE_PERP",
                    "api_key_env": "EQUITIES_BINANCE_API_KEY",
                    "api_secret_env": "EQUITIES_BINANCE_API_SECRET",
                    "account_type": "USDT_FUTURES",
                    "private_api_family": "PORTFOLIO_MARGIN",
                    "recv_window_ms": 5000,
                },
            ],
        )[0],
        account_scope_id="binance.futures.main",
        source_strategy_ids=("pltr_binance_perp_makerv4",),
    )

    assert provider is not None
    provider.refresh()
    snapshot = provider.snapshot()

    assert snapshot is not None
    cash_row = next(row for row in snapshot["rows"] if row.get("kind") != "position")
    assert cash_row["asset"] == "USDT"
    assert cash_row["total"] == "1000.0"
    assert cash_row["free"] == "975.0"


def test_equities_portfolio_runner_parses_portfolio_margin_cross_wallet_balances(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("EQUITIES_BINANCE_API_KEY", "binance-key")
    monkeypatch.setenv("EQUITIES_BINANCE_API_SECRET", "binance-secret")

    class _FakeBinanceFuturesAccountHttpAPI:
        def __init__(
            self,
            client: Any,
            clock: Any,
            account_type: Any,
            private_api_family: Any | None = None,
        ) -> None:
            _ = (client, clock, account_type, private_api_family)

        async def query_futures_account_info(self, recv_window: str | None = None) -> Any:
            assert recv_window == "5000"
            return SimpleNamespace(
                assets=[
                    SimpleNamespace(
                        asset="USDT",
                        crossWalletBalance="1000.0",
                        crossUnPnl="0.0",
                    ),
                    SimpleNamespace(
                        asset="USDC",
                        crossWalletBalance="0.0",
                        crossUnPnl="0.0",
                    ),
                ],
            )

        async def query_futures_position_risk(
            self,
            symbol: str | None = None,
            recv_window: str | None = None,
        ) -> list[Any]:
            assert symbol is None
            assert recv_window == "5000"
            return []

    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.get_cached_binance_http_client",
        lambda **kwargs: object(),
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.BinanceFuturesAccountHttpAPI",
        _FakeBinanceFuturesAccountHttpAPI,
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.BinanceSpotAccountHttpAPI",
        type(
            "_FakeBinanceSpotAccountHttpAPI",
            (),
            {
                "__init__": lambda self, client, clock, account_type: None,
                "query_spot_account_info": lambda self, recv_window=None: asyncio.sleep(0, result=SimpleNamespace(balances=[])),
            },
        ),
    )

    provider = build_account_projection_provider(
        scope_config=decode_account_scopes(
            [
                {
                    "scope_id": "binance.futures.main",
                    "provider": "binance",
                    "venue": "BINANCE_PERP",
                    "api_key_env": "EQUITIES_BINANCE_API_KEY",
                    "api_secret_env": "EQUITIES_BINANCE_API_SECRET",
                    "account_type": "USDT_FUTURES",
                    "private_api_family": "PORTFOLIO_MARGIN",
                    "recv_window_ms": 5000,
                },
            ],
        )[0],
        account_scope_id="binance.futures.main",
        source_strategy_ids=("pltr_binance_perp_makerv4",),
    )

    assert provider is not None
    provider.refresh()
    snapshot = provider.snapshot()

    assert snapshot is not None
    assert len(snapshot["rows"]) == 1
    cash_row = snapshot["rows"][0]
    assert cash_row["asset"] == "USDT"
    assert cash_row["total"] == "1000.0"
    assert cash_row["free"] == "1000.0"
    assert cash_row["row_id"] != "shared_account:acc:0"


def test_equities_portfolio_runner_marks_portfolio_margin_projection_healthy_when_isolated_margin_missing(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("EQUITIES_BINANCE_API_KEY", "binance-key")
    monkeypatch.setenv("EQUITIES_BINANCE_API_SECRET", "binance-secret")

    class _FakeBinanceFuturesAccountHttpAPI:
        def __init__(
            self,
            client: Any,
            clock: Any,
            account_type: Any,
            private_api_family: Any | None = None,
        ) -> None:
            _ = (client, clock, account_type, private_api_family)

        async def query_futures_account_info(self, recv_window: str | None = None) -> Any:
            assert recv_window == "5000"
            return SimpleNamespace(
                totalMarginBalance="1025.5",
                availableBalance="1000.0",
                maxWithdrawAmount="995.0",
                assets=[
                    SimpleNamespace(
                        asset="USDT",
                        walletBalance="1000.0",
                        availableBalance="975.0",
                    ),
                ],
            )

        async def query_futures_position_risk(
            self,
            symbol: str | None = None,
            recv_window: str | None = None,
        ) -> list[Any]:
            assert symbol is None
            assert recv_window == "5000"
            return [
                SimpleNamespace(
                    symbol="PLTRUSDT",
                    positionAmt="12.5",
                    entryPrice="95.0",
                    markPrice="96.0",
                    unRealizedProfit="12.5",
                    updateTime=1_700_000_000_000,
                ),
            ]

    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.get_cached_binance_http_client",
        lambda **kwargs: object(),
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.BinanceFuturesAccountHttpAPI",
        _FakeBinanceFuturesAccountHttpAPI,
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.BinanceSpotAccountHttpAPI",
        type(
            "_FakeBinanceSpotAccountHttpAPI",
            (),
            {
                "__init__": lambda self, client, clock, account_type: None,
                "query_spot_account_info": lambda self, recv_window=None: asyncio.sleep(0, result=SimpleNamespace(balances=[])),
            },
        ),
    )

    provider = build_account_projection_provider(
        scope_config=decode_account_scopes(
            [
                {
                    "scope_id": "binance.futures.main",
                    "provider": "binance",
                    "venue": "BINANCE_PERP",
                    "api_key_env": "EQUITIES_BINANCE_API_KEY",
                    "api_secret_env": "EQUITIES_BINANCE_API_SECRET",
                    "account_type": "USDT_FUTURES",
                    "private_api_family": "PORTFOLIO_MARGIN",
                    "recv_window_ms": 5000,
                },
            ],
        )[0],
        account_scope_id="binance.futures.main",
        source_strategy_ids=("pltr_binance_perp_makerv4",),
    )

    assert provider is not None
    provider.refresh()
    snapshot = provider.snapshot()

    assert snapshot is not None
    position_row = next(row for row in snapshot["rows"] if row.get("kind") == "position")
    assert position_row["instrument_id"] == "PLTRUSDT-PERP.BINANCE_PERP"
    assert snapshot["projection_status"]["healthy"] is True
    assert snapshot["projection_status"]["last_error_type"] is None


def test_equities_portfolio_runner_omits_empty_binance_shared_account_wrapper_row(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setenv("EQUITIES_BINANCE_API_KEY", "binance-key")
    monkeypatch.setenv("EQUITIES_BINANCE_API_SECRET", "binance-secret")

    class _FakeBinanceFuturesAccountHttpAPI:
        def __init__(
            self,
            client: Any,
            clock: Any,
            account_type: Any,
            private_api_family: Any | None = None,
        ) -> None:
            _ = (client, clock, account_type, private_api_family)

        async def query_futures_account_info(self, recv_window: str | None = None) -> Any:
            assert recv_window == "5000"
            return SimpleNamespace(
                assets=[
                    SimpleNamespace(
                        asset="USDT",
                        crossWalletBalance="0.00000000",
                        crossUnPnl="0.00000000",
                    ),
                ],
            )

        async def query_futures_position_risk(
            self,
            symbol: str | None = None,
            recv_window: str | None = None,
        ) -> list[Any]:
            assert symbol is None
            assert recv_window == "5000"
            return []

    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.get_cached_binance_http_client",
        lambda **kwargs: object(),
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.BinanceFuturesAccountHttpAPI",
        _FakeBinanceFuturesAccountHttpAPI,
    )
    monkeypatch.setattr(
        "flux.runners.shared.profile_accounts.BinanceSpotAccountHttpAPI",
        type(
            "_FakeBinanceSpotAccountHttpAPI",
            (),
            {
                "__init__": lambda self, client, clock, account_type: None,
                "query_spot_account_info": lambda self, recv_window=None: asyncio.sleep(0, result=SimpleNamespace(balances=[])),
            },
        ),
    )

    provider = build_account_projection_provider(
        scope_config=decode_account_scopes(
            [
                {
                    "scope_id": "binance.futures.main",
                    "provider": "binance",
                    "venue": "BINANCE_PERP",
                    "api_key_env": "EQUITIES_BINANCE_API_KEY",
                    "api_secret_env": "EQUITIES_BINANCE_API_SECRET",
                    "account_type": "USDT_FUTURES",
                    "private_api_family": "PORTFOLIO_MARGIN",
                    "recv_window_ms": 5000,
                },
            ],
        )[0],
        account_scope_id="binance.futures.main",
        source_strategy_ids=("pltr_binance_perp_makerv4",),
    )

    assert provider is not None
    provider.refresh()
    snapshot = provider.snapshot()

    assert snapshot is not None
    assert snapshot["rows"] == []


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


def test_equities_portfolio_aggregator_clears_stale_empty_account_projection() -> None:
    provider = _SequenceAccountProjectionProvider(
        snapshots=[
            {
                "rows": [
                    {
                        "exchange": "binance_perp",
                        "asset": "USDT",
                        "total": "1000",
                    },
                ],
                "totals": {},
            },
            {
                "rows": [],
                "totals": {},
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
    aggregator._aggregation_mode = "strict"
    aggregator._strategy_ids = []
    aggregator._required_strategy_ids = set()
    aggregator._base_assets = []
    aggregator._strategy_ids_by_asset = {}
    aggregator._redis = fake_redis
    aggregator._log = MagicMock()
    aggregator.account_scope_ids = ["binance.futures.main"]
    aggregator._profile_account_bindings = (
        ProfileAccountProviderBinding(
            account_scope_id="binance.futures.main",
            source_strategy_ids=("pltr_binance_perp_makerv4",),
            provider=provider,
        ),
    )

    projection_key = FluxRedisKeys.profile_account_projection(
        profile_id="equities",
        account_scope_id="binance.futures.main",
    )
    snapshot_key = FluxRedisKeys.portfolio_snapshot(portfolio_id="equities")

    aggregator.recompute_once()
    projection_raw = fake_redis.get(projection_key)
    assert projection_raw is not None

    first_snapshot_raw = fake_redis.get(snapshot_key)
    assert first_snapshot_raw is not None
    first_snapshot = json.loads(first_snapshot_raw)
    assert len(first_snapshot["accounts"]["rows"]) == 1

    aggregator.recompute_once()
    assert fake_redis.get(projection_key) is None

    snapshot_raw = fake_redis.get(snapshot_key)
    assert snapshot_raw is not None
    snapshot = json.loads(snapshot_raw)
    assert snapshot["accounts"]["rows"] == []
    assert provider.refresh_calls == 2


def test_equities_portfolio_aggregator_preserves_stale_projection_scope_after_refresh_failure() -> None:
    class ValidationError(Exception):
        pass

    class _FailingProjectionProvider:
        def __init__(self) -> None:
            self.refresh_calls = 0
            self._snapshot = {
                "source_scope": "shared_account",
                "rows": [
                    {
                        "exchange": "ibkr",
                        "account": "U1234567",
                        "asset": "USD",
                        "total": "1000",
                        "ts_ms": 1_700_000_000_000,
                        "stale": True,
                        "include_in_reconciliation": False,
                    },
                ],
                "totals": {"account_equity_raw": 1000.0},
                "projection_status": {
                    "healthy": False,
                    "last_success_ts_ms": 1_700_000_000_000,
                    "last_attempt_ts_ms": 1_700_000_015_000,
                    "last_error_type": "ValidationError",
                    "last_error_message": "bad binance payload",
                    "stale_after_ms": 15_000,
                },
            }

        def refresh(self) -> None:
            self.refresh_calls += 1
            raise ValidationError("bad binance payload")

        def snapshot(self) -> dict[str, Any]:
            return {
                "source_scope": self._snapshot["source_scope"],
                "rows": [dict(row) for row in self._snapshot["rows"]],
                "totals": dict(self._snapshot["totals"]),
                "projection_status": dict(self._snapshot["projection_status"]),
            }

    class _HealthyProjectionProvider:
        def __init__(self) -> None:
            self.refresh_calls = 0
            self._snapshot = {
                "source_scope": "shared_account",
                "rows": [
                    {
                        "exchange": "binance_perp",
                        "account": "BINANCE_PERP-master",
                        "asset": "USDT",
                        "total": "5000",
                        "ts_ms": 1_700_000_015_000,
                        "stale": False,
                        "include_in_reconciliation": True,
                    },
                ],
                "totals": {"withdrawable_raw": 5000.0},
                "projection_status": {
                    "healthy": True,
                    "last_success_ts_ms": 1_700_000_015_000,
                    "last_attempt_ts_ms": 1_700_000_015_000,
                    "last_error_type": None,
                    "last_error_message": None,
                    "stale_after_ms": 15_000,
                },
            }

        def refresh(self) -> None:
            self.refresh_calls += 1

        def snapshot(self) -> dict[str, Any]:
            return {
                "source_scope": self._snapshot["source_scope"],
                "rows": [dict(row) for row in self._snapshot["rows"]],
                "totals": dict(self._snapshot["totals"]),
                "projection_status": dict(self._snapshot["projection_status"]),
            }

    fake_redis = _FakeRedis()
    aggregator = EquitiesPortfolioAggregator.__new__(EquitiesPortfolioAggregator)
    aggregator._descriptor = get_strategy_set_descriptor("equities")
    aggregator._namespace = "flux"
    aggregator._schema_version = "v1"
    aggregator._mode = "live"
    aggregator._portfolio_id = "equities"
    aggregator._stale_after_ms = 3_000
    aggregator._aggregation_mode = "strict"
    aggregator._strategy_ids = []
    aggregator._required_strategy_ids = set()
    aggregator._base_assets = []
    aggregator._strategy_ids_by_asset = {}
    aggregator._redis = fake_redis
    aggregator._log = MagicMock()
    aggregator.account_scope_ids = ["ibkr.reference.main", "binance.futures.main"]
    aggregator._profile_account_bindings = (
        ProfileAccountProviderBinding(
            account_scope_id="ibkr.reference.main",
            source_strategy_ids=("intc_binance_perp_makerv4",),
            provider=_FailingProjectionProvider(),
        ),
        ProfileAccountProviderBinding(
            account_scope_id="binance.futures.main",
            source_strategy_ids=("intc_binance_perp_makerv4",),
            provider=_HealthyProjectionProvider(),
        ),
    )

    aggregator.recompute_once()

    ibkr_projection_raw = fake_redis.get(
        FluxRedisKeys.profile_account_projection(
            profile_id="equities",
            account_scope_id="ibkr.reference.main",
        ),
    )
    assert ibkr_projection_raw is not None
    ibkr_projection = json.loads(ibkr_projection_raw)
    assert ibkr_projection["rows"][0]["stale"] is True
    assert ibkr_projection["rows"][0]["include_in_reconciliation"] is False
    assert ibkr_projection["scope_status"] == [
        {
            "account_scope_id": "ibkr.reference.main",
            "source_scope": "shared_account",
            "projection_status": {
                "healthy": False,
                "last_success_ts_ms": 1_700_000_000_000,
                "last_attempt_ts_ms": 1_700_000_015_000,
                "last_error_type": "ValidationError",
                "last_error_message": "bad binance payload",
                "stale_after_ms": 15_000,
            },
        },
    ]

    portfolio_snapshot_raw = fake_redis.get(FluxRedisKeys.portfolio_snapshot(portfolio_id="equities"))
    assert portfolio_snapshot_raw is not None
    portfolio_snapshot = json.loads(portfolio_snapshot_raw)
    ibkr_rows = [
        row
        for row in portfolio_snapshot["accounts"]["rows"]
        if row.get("account_scope_id") == "ibkr.reference.main"
    ]
    assert len(ibkr_rows) == 1
    assert ibkr_rows[0]["stale"] is True
    assert ibkr_rows[0]["include_in_reconciliation"] is False
    assert portfolio_snapshot["accounts"]["scope_status"] == [
        {
            "account_scope_id": "ibkr.reference.main",
            "source_scope": "shared_account",
            "projection_status": {
                "healthy": False,
                "last_success_ts_ms": 1_700_000_000_000,
                "last_attempt_ts_ms": 1_700_000_015_000,
                "last_error_type": "ValidationError",
                "last_error_message": "bad binance payload",
                "stale_after_ms": 15_000,
            },
        },
        {
            "account_scope_id": "binance.futures.main",
            "source_scope": "shared_account",
            "projection_status": {
                "healthy": True,
                "last_success_ts_ms": 1_700_000_015_000,
                "last_attempt_ts_ms": 1_700_000_015_000,
                "last_error_type": None,
                "last_error_message": None,
                "stale_after_ms": 15_000,
            },
        },
    ]


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


def test_equities_portfolio_aggregator_nets_same_stock_multivenue_routes_into_one_bucket() -> None:
    now_ms_value = int(time.time() * 1000)
    provider = _CountingAccountProjectionProvider(
        rows=[
            {
                "exchange": "ibkr",
                "account": "U1234567",
                "asset": "USD",
                "total": "1000",
                "account_scope_id": "ibkr.reference.main",
                "source_scope": "shared_account",
            },
        ],
    )
    fake_redis = _FakeRedis(
        {
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="pltr_tradexyz_makerv4",
                portfolio_id="equities",
                base_currency="PLTR",
            ): encode_component(
                StrategyInventoryComponent(
                    strategy_id="pltr_tradexyz_makerv4",
                    portfolio_id="equities",
                    base_currency="PLTR",
                    local_qty_base=10,
                    local_position_qty_venue=10,
                    local_position_qty_base=10,
                    qty_conversion_status="identity",
                    qty_conversion_source="generic:multiplier=1",
                    ts_ms=now_ms_value,
                    maker_instrument_id="xyz:PLTR-USD-PERP.HYPERLIQUID",
                    state="running",
                ),
            ).encode(),
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="pltr_binance_perp_makerv4",
                portfolio_id="equities",
                base_currency="PLTR",
            ): encode_component(
                StrategyInventoryComponent(
                    strategy_id="pltr_binance_perp_makerv4",
                    portfolio_id="equities",
                    base_currency="PLTR",
                    local_qty_base=5,
                    local_position_qty_venue=80,
                    local_position_qty_base=5,
                    qty_conversion_status="converted",
                    qty_conversion_source="generic:multiplier=0.0625",
                    ts_ms=now_ms_value,
                    maker_instrument_id="PLTRUSDT-PERP.BINANCE_PERP",
                    state="running",
                ),
            ).encode(),
            FluxRedisKeys(strategy_id="pltr_tradexyz_makerv4").balances_snapshot(): json.dumps([]).encode(),
            FluxRedisKeys(strategy_id="pltr_binance_perp_makerv4").balances_snapshot(): json.dumps([]).encode(),
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
    aggregator._strategy_ids = ["pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4"]
    aggregator._required_strategy_ids = set(aggregator._strategy_ids)
    aggregator._base_assets = ["PLTR"]
    aggregator._strategy_ids_by_asset = {
        "PLTR": ("pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4"),
    }
    aggregator._redis = fake_redis
    aggregator._log = MagicMock()
    aggregator.account_scope_ids = ["ibkr.reference.main"]
    aggregator._profile_account_bindings = (
        ProfileAccountProviderBinding(
            account_scope_id="ibkr.reference.main",
            source_strategy_ids=("pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4"),
            provider=provider,
        ),
    )

    aggregator.recompute_once()

    raw_snapshot = fake_redis.get(FluxRedisKeys.portfolio_snapshot(portfolio_id="equities"))
    assert raw_snapshot is not None
    snapshot = json.loads(raw_snapshot)

    pltr_inventory = snapshot["inventory_by_asset"]["PLTR"]
    assert pltr_inventory["global_qty_base"] == "15.000000"
    assert pltr_inventory["global_qty_base_complete"] is True
    assert sorted(row["strategy_id"] for row in pltr_inventory["components"]) == [
        "pltr_binance_perp_makerv4",
        "pltr_tradexyz_makerv4",
    ]
    binance_component = next(
        row for row in pltr_inventory["components"] if row["strategy_id"] == "pltr_binance_perp_makerv4"
    )
    assert binance_component["local_position_qty_venue"] == "80.000000"
    assert binance_component["local_position_qty_base"] == "5.000000"
    assert binance_component["maker_instrument_id"] == "PLTRUSDT-PERP.BINANCE_PERP"
    assert snapshot["accounts"]["rows"][0]["source_strategy_ids"] == [
        "pltr_tradexyz_makerv4",
        "pltr_binance_perp_makerv4",
    ]


def test_equities_portfolio_aggregator_keeps_same_stock_multivenue_contributors_visible() -> None:
    now_ms_value = int(time.time() * 1000)
    provider = _CountingAccountProjectionProvider(
        rows=[
            {
                "exchange": "ibkr",
                "account": "U1234567",
                "asset": "USD",
                "free": "1000",
                "total": "1000",
                "account_scope_id": "ibkr.reference.main",
                "source_scope": "shared_account",
            },
        ],
    )
    fake_redis = _FakeRedis(
        {
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="pltr_tradexyz_makerv4",
                portfolio_id="equities",
                base_currency="PLTR",
            ): encode_component(
                StrategyInventoryComponent(
                    strategy_id="pltr_tradexyz_makerv4",
                    portfolio_id="equities",
                    base_currency="PLTR",
                    local_qty_base=10,
                    ts_ms=now_ms_value,
                    maker_instrument_id="xyz:PLTR-USD-PERP.HYPERLIQUID",
                    state="running",
                ),
            ).encode(),
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="pltr_binance_perp_makerv4",
                portfolio_id="equities",
                base_currency="PLTR",
            ): encode_component(
                StrategyInventoryComponent(
                    strategy_id="pltr_binance_perp_makerv4",
                    portfolio_id="equities",
                    base_currency="PLTR",
                    local_qty_base=-4,
                    ts_ms=now_ms_value,
                    maker_instrument_id="PLTRUSDT-PERP.BINANCE_PERP",
                    state="running",
                ),
            ).encode(),
            FluxRedisKeys(strategy_id="pltr_tradexyz_makerv4").balances_snapshot(): json.dumps(
                [
                    {
                        "strategy_id": "pltr_tradexyz_makerv4",
                        "exchange": "hyperliquid",
                        "kind": "position",
                        "asset": "PLTR",
                        "instrument_id": "xyz:PLTR-USD-PERP.HYPERLIQUID",
                        "signed_qty": "10",
                        "quantity": "10",
                        "mark_raw": 20.0,
                        "mv_raw": 200.0,
                        "ts_ms": now_ms_value,
                    },
                ],
            ).encode(),
            FluxRedisKeys(strategy_id="pltr_binance_perp_makerv4").balances_snapshot(): json.dumps(
                [
                    {
                        "strategy_id": "pltr_binance_perp_makerv4",
                        "exchange": "binance_perp",
                        "kind": "position",
                        "asset": "PLTR",
                        "instrument_id": "PLTRUSDT-PERP.BINANCE_PERP",
                        "signed_qty": "-4",
                        "quantity": "4",
                        "mark_raw": 20.0,
                        "mv_raw": -80.0,
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
    aggregator._strategy_ids = ["pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4"]
    aggregator._required_strategy_ids = set(aggregator._strategy_ids)
    aggregator._base_assets = ["PLTR"]
    aggregator._strategy_ids_by_asset = {
        "PLTR": ("pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4"),
    }
    aggregator._redis = fake_redis
    aggregator._log = MagicMock()
    aggregator.account_scope_ids = ["ibkr.reference.main"]
    aggregator._profile_account_bindings = (
        ProfileAccountProviderBinding(
            account_scope_id="ibkr.reference.main",
            source_strategy_ids=("pltr_tradexyz_makerv4", "pltr_binance_perp_makerv4"),
            provider=provider,
        ),
    )

    aggregator.recompute_once()

    inventory = decode_portfolio_inventory(
        fake_redis.get(
            FluxRedisKeys.portfolio_inventory(portfolio_id="equities", base_currency="PLTR"),
        ),
    )
    raw_snapshot = fake_redis.get(FluxRedisKeys.portfolio_snapshot(portfolio_id="equities"))

    assert inventory is not None
    assert inventory["global_qty_base"] == "6.000000"
    assert [row["strategy_id"] for row in inventory["components"]] == [
        "pltr_binance_perp_makerv4",
        "pltr_tradexyz_makerv4",
    ]
    assert raw_snapshot is not None

    snapshot = json.loads(raw_snapshot)
    assert snapshot["inventory_by_asset"]["PLTR"]["global_qty_base"] == "6.000000"
    assert [row["strategy_id"] for row in snapshot["inventory_by_asset"]["PLTR"]["components"]] == [
        "pltr_binance_perp_makerv4",
        "pltr_tradexyz_makerv4",
    ]
    assert {row["instrument_id"].upper() for row in snapshot["balances"]["rows"]} == {
        "PLTRUSDT-PERP.BINANCE_PERP",
        "XYZ:PLTR-USD-PERP.HYPERLIQUID",
    }
    account_row = snapshot["accounts"]["rows"][0]
    assert account_row["source_scope"] == "shared_account"
    assert account_row["account_scope_id"] == "ibkr.reference.main"
    assert account_row["source_strategy_ids"] == [
        "pltr_tradexyz_makerv4",
        "pltr_binance_perp_makerv4",
    ]


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
