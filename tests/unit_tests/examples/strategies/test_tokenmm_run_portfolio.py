from __future__ import annotations

import json
import sqlite3
import time
from sqlite3 import OperationalError
from typing import Any
from unittest.mock import MagicMock

import pytest
from flux.common.keys import FluxRedisKeys
from flux.common.portfolio_inventory import StrategyInventoryComponent
from flux.common.portfolio_inventory import decode_portfolio_inventory
from flux.common.portfolio_inventory import encode_component
from flux.runners.shared.portfolio_runner import parse_required_strategy_ids
from flux.runners.shared.portfolio_runner import parse_strategy_ids
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.runners.tokenmm.run_portfolio import TokenMMPortfolioAggregator
from flux.runners.tokenmm.run_portfolio import _portfolio_base_assets


class _FakePipeline:
    def __init__(self, redis_client: "_FakeRedis") -> None:
        self._redis = redis_client
        self._keys: list[str] = []

    def get(self, key: str) -> "_FakePipeline":
        self._keys.append(key)
        return self

    def execute(self) -> list[bytes | None]:
        return [self._redis.get(key) for key in self._keys]


class _FakeConnectionPool:
    def __init__(self) -> None:
        self.disconnect_calls: list[bool] = []

    def disconnect(self, *, in_use_connections: bool = True) -> None:
        self.disconnect_calls.append(in_use_connections)


class _FakeRedis:
    def __init__(self, values: dict[str, bytes | None] | None = None) -> None:
        self.values = dict(values or {})
        self.published: list[tuple[str, str]] = []
        self.closed = False
        self.connection_pool = _FakeConnectionPool()

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


def test_portfolio_base_assets_dedupes_contract_bases() -> None:
    assert _portfolio_base_assets(
        {
            "contracts": [
                {"exchange": "bybit", "symbol": "PLUME/USDT"},
                {"exchange": "okx", "symbol": "PLUME/USDT"},
            ],
        },
    ) == ["PLUME"]


def test_tokenmm_portfolio_allowlist_uses_shared_parser() -> None:
    descriptor = get_strategy_set_descriptor("tokenmm")

    assert parse_strategy_ids(
        {"tokenmm_strategy_ids": ["plumeusdt_bybit_perp_makerv3", "plumeusdt_okx_perp_makerv3"]},
        descriptor=descriptor,
    ) == ["plumeusdt_bybit_perp_makerv3", "plumeusdt_okx_perp_makerv3"]
    assert parse_required_strategy_ids(
        {"tokenmm_required_strategy_ids": ["plumeusdt_bybit_perp_makerv3"]},
        descriptor=descriptor,
        fallback=["plumeusdt_bybit_perp_makerv3", "plumeusdt_okx_perp_makerv3"],
    ) == ["plumeusdt_bybit_perp_makerv3"]


def test_portfolio_aggregator_sums_allowlisted_component_keys() -> None:
    now_ms_value = int(time.time() * 1000)
    config: dict[str, Any] = {
        "flux": {"namespace": "flux", "schema_version": "v1"},
        "redis": {},
        "api": {
            "tokenmm_strategy_ids": [
                "plumeusdt_bybit_perp_makerv3",
                "plumeusdt_okx_perp_makerv3",
            ],
            "tokenmm_required_strategy_ids": [
                "plumeusdt_bybit_perp_makerv3",
                "plumeusdt_okx_perp_makerv3",
            ],
        },
        "portfolio": {"portfolio_id": "tokenmm"},
        "contracts": [{"exchange": "bybit", "symbol": "PLUME/USDT"}],
    }
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
                        local_qty_base=36689,
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
                        local_qty_base=-9806,
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

    aggregator.recompute_once()

    payload = decode_portfolio_inventory(
        fake_redis.get(FluxRedisKeys.portfolio_inventory(portfolio_id="tokenmm", base_currency="PLUME")),
    )

    assert payload is not None
    assert payload["global_qty_base"] == "26883.000000"
    assert payload["global_qty"] == "26883.000000"
    assert payload["aggregation_mode"] == "strict"
    assert payload["global_qty_base_complete"] is True
    assert payload["global_qty_complete"] is True
    assert payload["missing_required"] == []
    assert fake_redis.published


def test_portfolio_aggregator_partial_mode_keeps_partial_sum_for_missing_required() -> None:
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
                    local_qty_base=36689,
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
    aggregator._aggregation_mode = "partial"
    aggregator._strategy_ids = [
        "plumeusdt_bybit_perp_makerv3",
        "plumeusdt_okx_perp_makerv3",
    ]
    aggregator._required_strategy_ids = set(aggregator._strategy_ids)
    aggregator._base_assets = ["PLUME"]
    aggregator._redis = fake_redis
    aggregator._log = None

    aggregator.recompute_once()

    payload = decode_portfolio_inventory(
        fake_redis.get(FluxRedisKeys.portfolio_inventory(portfolio_id="tokenmm", base_currency="PLUME")),
    )

    assert payload is not None
    assert payload["global_qty_base"] == "36689.000000"
    assert payload["global_qty"] == "36689.000000"
    assert payload["aggregation_mode"] == "partial"
    assert payload["global_qty_base_complete"] is False
    assert payload["global_qty_complete"] is False
    assert payload["missing_required"] == ["plumeusdt_okx_perp_makerv3"]


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
                    local_qty_base=36689,
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


def test_tokenmm_portfolio_aggregator_publishes_portfolio_snapshot_with_balances_and_partial_inventory() -> None:
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
                    local_qty_base=10,
                    ts_ms=now_ms_value,
                    state="running",
                ),
            ).encode(),
            FluxRedisKeys(strategy_id="plumeusdt_bybit_perp_makerv3").balances_snapshot(): json.dumps(
                [
                    {
                        "strategy_id": "plumeusdt_bybit_perp_makerv3",
                        "exchange": "bybit",
                        "asset": "PLUME",
                        "account": "trading",
                        "total": "10",
                        "ts_ms": now_ms_value,
                        "mark_raw": 1.5,
                        "mv_raw": 15.0,
                    },
                ],
            ).encode(),
            FluxRedisKeys(strategy_id="plumeusdt_okx_perp_makerv3").balances_snapshot(): json.dumps(
                [
                    {
                        "strategy_id": "plumeusdt_okx_perp_makerv3",
                        "exchange": "okx",
                        "asset": "USDT",
                        "account": "trading",
                        "total": "3",
                        "ts_ms": now_ms_value,
                        "mark_raw": 1.0,
                        "mv_raw": 3.0,
                    },
                ],
            ).encode(),
        },
    )
    aggregator = TokenMMPortfolioAggregator.__new__(TokenMMPortfolioAggregator)
    aggregator._namespace = "flux"
    aggregator._schema_version = "v1"
    aggregator._mode = "live"
    aggregator._portfolio_id = "tokenmm"
    aggregator._stale_after_ms = 3_000
    aggregator._aggregation_mode = "partial"
    aggregator._strategy_ids = [
        "plumeusdt_bybit_perp_makerv3",
        "plumeusdt_okx_perp_makerv3",
    ]
    aggregator._required_strategy_ids = set(aggregator._strategy_ids)
    aggregator._base_assets = ["PLUME"]
    aggregator._redis = fake_redis
    aggregator._log = MagicMock()

    aggregator.recompute_once()

    raw_snapshot = fake_redis.get(FluxRedisKeys.portfolio_snapshot(portfolio_id="tokenmm"))
    assert raw_snapshot is not None
    snapshot = json.loads(raw_snapshot)

    assert snapshot["inventory"]["aggregation_mode"] == "partial"
    assert snapshot["inventory"]["global_qty_base"] == "10.000000"
    assert snapshot["inventory"]["global_qty"] == "10.000000"
    assert snapshot["inventory"]["global_qty_base_complete"] is False
    assert snapshot["inventory"]["global_qty_complete"] is False
    assert snapshot["inventory"]["missing_required"] == ["plumeusdt_okx_perp_makerv3"]
    assert all(row["strategy_id"] == "tokenmm" for row in snapshot["balances"]["rows"])
    assert snapshot["balances"]["totals"]["mv_raw"] == 18.0
    assert (
        FluxRedisKeys.portfolio_snapshot_channel(portfolio_id="tokenmm"),
        raw_snapshot.decode(),
    ) in fake_redis.published


def test_tokenmm_portfolio_aggregator_keeps_redis_publication_when_snapshot_persistence_fails() -> None:
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
                    local_qty_base=10,
                    ts_ms=now_ms_value,
                    state="running",
                ),
            ).encode(),
        },
    )

    class _FailingSnapshotWriter:
        def maybe_persist(self, *, payload, ts_ms):
            raise OperationalError("database is locked")

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
    aggregator._log = MagicMock()
    aggregator._snapshot_writer = _FailingSnapshotWriter()

    aggregator.recompute_once()

    payload = decode_portfolio_inventory(
        fake_redis.get(FluxRedisKeys.portfolio_inventory(portfolio_id="tokenmm", base_currency="PLUME")),
    )
    assert payload is not None
    assert payload["global_qty_base"] == "10.000000"
    aggregator._log.warning.assert_called()


def test_tokenmm_portfolio_aggregator_threads_inventory_aggregation_mode_from_config(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(
        "flux.runners.shared.portfolio_runner.build_redis_client",
        lambda _cfg: _FakeRedis(),
    )
    base_config: dict[str, Any] = {
        "flux": {"namespace": "flux", "schema_version": "v1"},
        "redis": {},
        "api": {"tokenmm_strategy_ids": ["plumeusdt_bybit_perp_makerv3"]},
        "portfolio": {"portfolio_id": "tokenmm"},
        "contracts": [{"exchange": "bybit", "symbol": "PLUME/USDT"}],
    }

    strict_aggregator = TokenMMPortfolioAggregator(
        config=base_config,
        mode="paper",
        logger=MagicMock(),
    )
    partial_aggregator = TokenMMPortfolioAggregator(
        config={
            **base_config,
            "portfolio": {
                **base_config["portfolio"],
                "inventory_aggregation_mode": "partial",
            },
        },
        mode="paper",
        logger=MagicMock(),
    )

    assert strict_aggregator._aggregation_mode == "strict"
    assert partial_aggregator._aggregation_mode == "partial"


def test_portfolio_aggregator_run_closes_redis_on_exit(monkeypatch) -> None:
    aggregator = TokenMMPortfolioAggregator.__new__(TokenMMPortfolioAggregator)
    aggregator._descriptor = get_strategy_set_descriptor("tokenmm")
    aggregator._portfolio_id = "tokenmm"
    aggregator._mode = "live"
    aggregator._base_assets = ["PLUME"]
    aggregator._strategy_ids = ["plumeusdt_bybit_perp_makerv3"]
    aggregator._redis = _FakeRedis()
    aggregator._log = MagicMock()
    aggregator._running = True

    def _recompute_once() -> None:
        aggregator.stop()

    aggregator.recompute_once = _recompute_once
    monkeypatch.setattr(time, "sleep", lambda _secs: None)

    aggregator.run()

    assert aggregator._redis.closed is True
    assert aggregator._redis.connection_pool.disconnect_calls == [False]
