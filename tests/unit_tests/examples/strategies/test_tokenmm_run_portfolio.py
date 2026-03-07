from __future__ import annotations

import sqlite3
import sys
import time
from types import SimpleNamespace


try:
    import redis as _redis  # noqa: F401
except ModuleNotFoundError:  # pragma: no cover - local test harness fallback
    sys.modules["redis"] = SimpleNamespace(Redis=None)

from flux.common.keys import FluxRedisKeys
from flux.common.portfolio_inventory import StrategyInventoryComponent
from flux.common.portfolio_inventory import decode_portfolio_inventory
from flux.common.portfolio_inventory import encode_component
from flux.runners.tokenmm.run_portfolio import TokenMMPortfolioAggregator
from flux.runners.tokenmm.run_portfolio import _portfolio_base_assets


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


def test_portfolio_base_assets_dedupes_contract_bases() -> None:
    assert _portfolio_base_assets(
        {
            "contracts": [
                {"exchange": "bybit", "symbol": "PLUME/USDT"},
                {"exchange": "okx", "symbol": "PLUME/USDT"},
            ],
        },
    ) == ["PLUME"]


def test_portfolio_aggregator_sums_allowlisted_component_keys() -> None:
    now_ms_value = int(time.time() * 1000)
    fake_redis = _FakeRedis(
        {
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="plumeusdt_bybit_perp_makerv3",
                portfolio_id="tokenmm",
                base_currency="PLUME",
            ): encode_component(
                    StrategyInventoryComponent(
                        strategy_id="plumeusdt_bybit_perp_makerv3",
                        portfolio_id="tokenmm",
                        base_currency="PLUME",
                        local_qty=36689,
                        ts_ms=now_ms_value,
                        state="running",
                    ),
                ).encode(),
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="plumeusdt_okx_perp_makerv3",
                portfolio_id="tokenmm",
                base_currency="PLUME",
            ): encode_component(
                    StrategyInventoryComponent(
                        strategy_id="plumeusdt_okx_perp_makerv3",
                        portfolio_id="tokenmm",
                        base_currency="PLUME",
                        local_qty=-9806,
                        ts_ms=now_ms_value,
                        state="running",
                    ),
                ).encode(),
        },
    )
    aggregator = TokenMMPortfolioAggregator.__new__(TokenMMPortfolioAggregator)
    aggregator._namespace = "flux"
    aggregator._schema_version = "v1"
    aggregator._mode = "live"
    aggregator._portfolio_id = "tokenmm"
    aggregator._stale_after_ms = 3_000
    aggregator._strategy_ids = [
        "plumeusdt_bybit_perp_makerv3",
        "plumeusdt_okx_perp_makerv3",
    ]
    aggregator._required_strategy_ids = set(aggregator._strategy_ids)
    aggregator._base_assets = ["PLUME"]
    aggregator._redis = fake_redis
    aggregator._log = None
    aggregator._snapshot_writer = None

    aggregator.recompute_once()

    payload = decode_portfolio_inventory(
        fake_redis.get(FluxRedisKeys.portfolio_inventory(portfolio_id="tokenmm", base_currency="PLUME")),
    )

    assert payload is not None
    assert payload["global_qty"] == "26883.000000"
    assert payload["missing_required"] == []
    assert fake_redis.published


def test_portfolio_aggregator_persists_inventory_history_when_writer_is_configured(tmp_path) -> None:
    from nautilus_trader.flux.persistence.portfolio_inventory_snapshots.sqlite import (
        PortfolioInventorySnapshotWriter,
    )

    now_ms_value = int(time.time() * 1000)
    fake_redis = _FakeRedis(
        {
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id="plumeusdt_bybit_perp_makerv3",
                portfolio_id="tokenmm",
                base_currency="PLUME",
            ): encode_component(
                StrategyInventoryComponent(
                    strategy_id="plumeusdt_bybit_perp_makerv3",
                    portfolio_id="tokenmm",
                    base_currency="PLUME",
                    local_qty=36689,
                    ts_ms=now_ms_value,
                    state="running",
                ),
            ).encode(),
        },
    )
    aggregator = TokenMMPortfolioAggregator.__new__(TokenMMPortfolioAggregator)
    aggregator._namespace = "flux"
    aggregator._schema_version = "v1"
    aggregator._mode = "live"
    aggregator._portfolio_id = "tokenmm"
    aggregator._stale_after_ms = 3_000
    aggregator._strategy_ids = ["plumeusdt_bybit_perp_makerv3"]
    aggregator._required_strategy_ids = set(aggregator._strategy_ids)
    aggregator._base_assets = ["PLUME"]
    aggregator._redis = fake_redis
    aggregator._log = None
    aggregator._snapshot_writer = PortfolioInventorySnapshotWriter(
        db_path=str(tmp_path / "portfolio_inventory.sqlite"),
        unchanged_heartbeat_ms=5_000,
    )

    try:
        aggregator.recompute_once()
        aggregator.recompute_once()
    finally:
        aggregator._snapshot_writer.close()

    conn = sqlite3.connect(tmp_path / "portfolio_inventory.sqlite")
    try:
        count = conn.execute("SELECT COUNT(*) FROM portfolio_inventory_snapshot").fetchone()[0]
    finally:
        conn.close()

    assert count == 1
