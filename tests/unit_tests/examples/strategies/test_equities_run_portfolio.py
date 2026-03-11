from __future__ import annotations

import json
import time
from typing import Any
from unittest.mock import MagicMock

import pytest

from flux.common.account_projection import ProfileAccountProviderBinding
from flux.common.keys import FluxRedisKeys
from flux.common.portfolio_inventory import StrategyInventoryComponent
from flux.common.portfolio_inventory import decode_portfolio_inventory
from flux.common.portfolio_inventory import encode_component
from flux.runners.equities.run_portfolio import EquitiesPortfolioAggregator
from flux.runners.equities.run_portfolio import _equities_strategy_ids
from flux.runners.equities.run_portfolio import _portfolio_base_assets
from flux.runners.equities.run_portfolio import _required_strategy_ids
from flux.runners.shared.portfolio_runner import parse_required_strategy_ids
from flux.runners.shared.portfolio_runner import parse_strategy_ids
from flux.runners.shared.strategy_set import get_strategy_set_descriptor


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
    ) -> None:
        self._rows = rows
        self.refresh_calls = 0

    def refresh(self) -> None:
        self.refresh_calls += 1

    def snapshot(self) -> dict[str, Any] | None:
        return {
            "rows": list(self._rows),
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


def test_equities_strategy_ids_requires_non_empty_allowlist() -> None:
    with pytest.raises(ValueError, match="non-empty"):
        _equities_strategy_ids({})


def test_required_strategy_ids_falls_back_to_allowlist() -> None:
    allowlist = ["aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"]

    assert _required_strategy_ids({}, fallback=allowlist) == allowlist


def test_equities_portfolio_allowlist_uses_shared_parser() -> None:
    descriptor = get_strategy_set_descriptor("equities")

    assert parse_strategy_ids(
        {"equities_strategy_ids": ["aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"]},
        descriptor=descriptor,
    ) == ["aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"]
    assert parse_required_strategy_ids(
        {"equities_required_strategy_ids": ["aapl_tradexyz_makerv3"]},
        descriptor=descriptor,
        fallback=["aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"],
    ) == ["aapl_tradexyz_makerv3"]


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


def test_portfolio_aggregator_sums_allowlisted_component_keys() -> None:
    now_ms_value = int(time.time() * 1000)
    fake_redis = _FakeRedis(
        {
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="aapl_tradexyz_makerv3",
                portfolio_id="equities",
                base_currency="AAPL",
            ): encode_component(
                StrategyInventoryComponent(
                    strategy_id="aapl_tradexyz_makerv3",
                    portfolio_id="equities",
                    base_currency="AAPL",
                    local_qty_base=15,
                    ts_ms=now_ms_value,
                    state="running",
                ),
            ).encode(),
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="msft_tradexyz_makerv3",
                portfolio_id="equities",
                base_currency="AAPL",
            ): encode_component(
                StrategyInventoryComponent(
                    strategy_id="msft_tradexyz_makerv3",
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
    aggregator._strategy_ids = ["aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"]
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


def test_equities_portfolio_runner_collects_shared_account_snapshots_once_per_scope(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        "flux.runners.shared.portfolio_runner.build_redis_client",
        lambda _cfg: _FakeRedis(),
    )
    config: dict[str, Any] = {
        "flux": {"namespace": "flux", "schema_version": "v1"},
        "redis": {},
        "venues": {"reference_venue": "IBKR"},
        "node": {
            "venues": {
                "IBKR": {
                    "adapter": "interactive_brokers",
                    "ibg_client_id": 7,
                },
            },
        },
        "api": {
            "equities_strategy_ids": ["aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"],
        },
        "portfolio": {"portfolio_id": "equities"},
        "contracts": [{"exchange": "hyperliquid", "symbol": "AAPL/USD"}],
        "strategy_contracts": [
            _strategy_contract(
                "aapl_tradexyz_makerv3",
                reference_account_scope_id="ibkr.reference.main",
            ),
            _strategy_contract(
                "msft_tradexyz_makerv3",
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
            source_strategy_ids=("aapl_tradexyz_makerv3", "msft_tradexyz_makerv3"),
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


def test_equities_portfolio_aggregator_run_closes_redis_on_exit_with_legacy_disconnect(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    aggregator = EquitiesPortfolioAggregator.__new__(EquitiesPortfolioAggregator)
    aggregator._descriptor = get_strategy_set_descriptor("equities")
    aggregator._portfolio_id = "equities"
    aggregator._mode = "paper"
    aggregator._base_assets = ["AAPL"]
    aggregator._strategy_ids = ["aapl_tradexyz_makerv3"]
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
