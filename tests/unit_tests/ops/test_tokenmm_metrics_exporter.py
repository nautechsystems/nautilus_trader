from __future__ import annotations

import importlib.util
import json
import sys
from dataclasses import dataclass
from decimal import Decimal
from pathlib import Path
from types import ModuleType

import pytest


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def _load_exporter_module() -> ModuleType:
    path = _repo_root() / "ops/scripts/exporters/tokenmm_metrics_exporter.py"
    assert path.exists(), "liquidity exporter script should exist"

    spec = importlib.util.spec_from_file_location("task2_tokenmm_metrics_exporter", path)
    assert spec is not None
    assert spec.loader is not None

    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


@dataclass
class FakeRedis:
    values: dict[str, str]

    def __init__(self) -> None:
        self.values = {}

    def get(self, key: str):
        return self.values.get(key)

    def set(self, key: str, value):
        self.values[key] = value if isinstance(value, str) else str(value)
        return True


def _row_json(**kwargs) -> str:
    return json.dumps(kwargs, separators=(",", ":"))


def test_compute_quote_up_logic() -> None:
    module = _load_exporter_module()

    now_ms = 1_700_000_000_000
    assert module.compute_quote_up("QUOTING", now_ms - 1_000, now_ms, 30_000) == 1
    assert module.compute_quote_up("PAUSED", now_ms - 1_000, now_ms, 30_000) == 0
    assert module.compute_quote_up("QUOTING", now_ms - 30_001, now_ms, 30_000) == 0


def test_discover_strategy_contexts_filters_by_group_and_normalizes_venues(tmp_path) -> None:
    module = _load_exporter_module()

    cfg_dir = tmp_path / "configs"
    cfg_dir.mkdir(parents=True, exist_ok=True)
    (cfg_dir / "strategies.ini").write_text(
        "\n".join(
            [
                "[strategy:bybit_binance_plumeusdt_makerv3]",
                "strategy_groups = tokenmm",
                "exchange = bybit_linear",
                "base_asset = PLUME",
                "quote_asset = USDT",
                "",
                "[strategy:okx_binance_plumeusdt_spot_makerv3]",
                "strategy_groups = tokenmm",
                "exchange = okx",
                "base_asset = PLUME",
                "quote_asset = USDT",
                "",
                "[strategy:not_tokenmm]",
                "strategy_groups = default",
                "exchange = bybit_linear",
                "base_asset = BTC",
                "quote_asset = USDT",
                "",
            ]
        ),
        encoding="utf-8",
    )

    contexts = module.discover_strategy_contexts(
        config_dir=str(cfg_dir),
        strategy_group="tokenmm",
    )

    assert set(contexts.keys()) == {
        "bybit_binance_plumeusdt_makerv3",
        "okx_binance_plumeusdt_spot_makerv3",
    }
    assert contexts["bybit_binance_plumeusdt_makerv3"].venue == "bybit_linear"
    assert contexts["bybit_binance_plumeusdt_makerv3"].symbol == "PLUME/USDT"
    assert contexts["okx_binance_plumeusdt_spot_makerv3"].venue == "okx_spot"
    assert contexts["okx_binance_plumeusdt_spot_makerv3"].symbol == "PLUME/USDT"


def test_depth_within_bps_uses_bid_and_ask_and_filters_distance() -> None:
    module = _load_exporter_module()

    maker_orders = {
        "bid": [
            {"px": "100", "rem_qty": "2", "status": "OPEN"},
            {"px": "99.8", "rem_qty": "1", "status": "CANCELED"},
        ],
        "ask": [
            {"px": "102", "rem_qty": "1", "status": "LIVE"},
            {"px": "103", "rem_qty": "1"},
        ],
    }

    depth_100 = module.compute_depth_usd_within_bps(
        maker_orders=maker_orders,
        top_bid=Decimal("100"),
        top_ask=Decimal("102"),
        bps_limit=100,
    )
    depth_200 = module.compute_depth_usd_within_bps(
        maker_orders=maker_orders,
        top_bid=Decimal("100"),
        top_ask=Decimal("102"),
        bps_limit=200,
    )

    assert depth_100 == Decimal("302")
    assert depth_200 == Decimal("405")


def test_quote_state_poll_exports_tokenmm_metric_names() -> None:
    module = _load_exporter_module()

    strategy_id = "bybit_binance_plumeusdt_makerv3"
    redis_client = FakeRedis()
    exporter = module.TokenMMMetricsExporter(
        redis_client=redis_client,
        env="prod",
        strategy_ids=[strategy_id],
    )
    now_ms = 1_700_000_000_000

    redis_client.set(
        f"maker_arb:{strategy_id}:state",
        _row_json(
            ts_ms=now_ms - 1_000,
            mode="QUOTING",
            maker_leg={"exchange": "bybit_linear", "symbol": "PLUME_USDT"},
            quote_snapshot={"maker_top_bid": "100", "maker_top_ask": "102"},
            maker_orders={
                "bid": [{"px": "100", "rem_qty": "2", "status": "OPEN"}],
                "ask": [{"px": "102", "rem_qty": "1", "status": "LIVE"}],
            },
        ),
    )

    exporter.poll_quote_states(now_ms=now_ms)
    labels = {
        "env": "prod",
        "token": "PLUME",
        "venue": "bybit_linear",
        "symbol": "PLUME/USDT",
        "strategy_family": "maker_v3",
    }
    assert exporter.registry.get_sample_value("tokenmm_quote_up", labels) == 1.0
    assert (
        exporter.registry.get_sample_value("tokenmm_quote_depth_usd_100bps", labels) == 302.0
    )
    assert (
        exporter.registry.get_sample_value("tokenmm_quote_depth_usd_200bps", labels) == 302.0
    )


def test_exporter_source_uses_existing_redis_state_contract() -> None:
    path = _repo_root() / "ops/scripts/exporters/tokenmm_metrics_exporter.py"
    assert path.exists(), "liquidity exporter script should exist"

    source = path.read_text(encoding="utf-8")

    assert "maker_arb:" in source
    assert "flux.strategies" not in source
    assert "flux.runners" not in source


def test_poll_quote_states_removes_stale_fallback_labels_after_context_update() -> None:
    module = _load_exporter_module()

    strategy_id = "bybit_binance_plumeusdt_makerv3"
    redis_client = FakeRedis()
    exporter = module.TokenMMMetricsExporter(
        redis_client=redis_client,
        env="prod",
        strategy_ids=[strategy_id],
    )
    now_ms = 1_700_000_000_000

    fallback_labels = {
        "env": "prod",
        "token": "PLUME",
        "venue": "bybit_spot",
        "symbol": "PLUME/USDT",
        "strategy_family": "maker_v3",
    }
    live_labels = {
        "env": "prod",
        "token": "PLUME",
        "venue": "bybit_linear",
        "symbol": "PLUME/USDT",
        "strategy_family": "maker_v3",
    }

    assert exporter.registry.get_sample_value("tokenmm_quote_up", fallback_labels) == 0.0
    assert exporter.registry.get_sample_value("tokenmm_quote_depth_usd_100bps", fallback_labels) == 0.0
    assert exporter.registry.get_sample_value("tokenmm_quote_depth_usd_200bps", fallback_labels) == 0.0

    redis_client.set(
        f"maker_arb:{strategy_id}:state",
        _row_json(
            ts_ms=now_ms - 1_000,
            mode="QUOTING",
            maker_leg={"exchange": "bybit_linear", "symbol": "PLUME_USDT"},
            quote_snapshot={"maker_top_bid": "100", "maker_top_ask": "102"},
            maker_orders={
                "bid": [{"px": "100", "rem_qty": "2", "status": "OPEN"}],
                "ask": [{"px": "102", "rem_qty": "1", "status": "LIVE"}],
            },
        ),
    )

    exporter.poll_quote_states(now_ms=now_ms)

    assert exporter.registry.get_sample_value("tokenmm_quote_up", fallback_labels) is None
    assert exporter.registry.get_sample_value("tokenmm_quote_depth_usd_100bps", fallback_labels) is None
    assert exporter.registry.get_sample_value("tokenmm_quote_depth_usd_200bps", fallback_labels) is None
    assert exporter.registry.get_sample_value("tokenmm_quote_up", live_labels) == 1.0
    assert exporter.registry.get_sample_value("tokenmm_quote_depth_usd_100bps", live_labels) == 302.0
    assert exporter.registry.get_sample_value("tokenmm_quote_depth_usd_200bps", live_labels) == 302.0


def test_poll_quote_states_keeps_other_strategies_live_when_one_redis_read_fails(caplog) -> None:
    module = _load_exporter_module()

    healthy_strategy_id = "okx_binance_plumeusdt_makerv3"
    failed_strategy_id = "bybit_binance_plumeusdt_makerv3"
    now_ms = 1_700_000_000_000

    class FlakyRedis(FakeRedis):
        def get(self, key: str):
            if key == f"maker_arb:{failed_strategy_id}:state":
                raise RuntimeError("redis unavailable")
            return super().get(key)

    redis_client = FlakyRedis()
    redis_client.set(
        f"maker_arb:{healthy_strategy_id}:state",
        _row_json(
            ts_ms=now_ms - 1_000,
            mode="QUOTING",
            maker_leg={"exchange": "okx_spot", "symbol": "PLUME_USDT"},
            quote_snapshot={"maker_top_bid": "100", "maker_top_ask": "102"},
            maker_orders={
                "bid": [{"px": "100", "rem_qty": "2", "status": "OPEN"}],
                "ask": [{"px": "102", "rem_qty": "1", "status": "LIVE"}],
            },
        ),
    )
    exporter = module.TokenMMMetricsExporter(
        redis_client=redis_client,
        env="prod",
        strategy_ids=[failed_strategy_id, healthy_strategy_id],
    )

    with caplog.at_level("ERROR"):
        exporter.poll_quote_states(now_ms=now_ms)

    labels = {
        "env": "prod",
        "token": "PLUME",
        "venue": "okx_spot",
        "symbol": "PLUME/USDT",
        "strategy_family": "maker_v3",
    }
    assert exporter.registry.get_sample_value("tokenmm_quote_up", labels) == 1.0
    assert exporter.registry.get_sample_value("tokenmm_quote_depth_usd_100bps", labels) == 302.0
    assert "failed to poll strategy state" in caplog.text
    assert failed_strategy_id in caplog.text


def test_build_parser_rejects_invalid_poll_configuration() -> None:
    module = _load_exporter_module()
    parser = module._build_parser()

    with pytest.raises(SystemExit):
        parser.parse_args(["--poll-interval-s", "0"])
    with pytest.raises(SystemExit):
        parser.parse_args(["--poll-interval-s", "0.4"])
    with pytest.raises(SystemExit):
        parser.parse_args(["--state-stale-ms", "-1"])
