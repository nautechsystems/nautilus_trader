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
    hashes: dict[str, dict[str, str]]

    def __init__(self) -> None:
        self.values = {}
        self.hashes = {}

    def get(self, key: str):
        return self.values.get(key)

    def set(self, key: str, value):
        self.values[key] = value if isinstance(value, str) else str(value)
        return True

    def hset(self, key: str, mapping: dict[str, object]):
        bucket = self.hashes.setdefault(key, {})
        for field, value in mapping.items():
            bucket[str(field)] = value if isinstance(value, str) else str(value)
        return True

    def hgetall(self, key: str):
        return dict(self.hashes.get(key, {}))


def _row_json(**kwargs) -> str:
    return json.dumps(kwargs, separators=(",", ":"))


def test_compute_quote_up_logic() -> None:
    module = _load_exporter_module()

    now_ms = 1_700_000_000_000
    assert module.compute_quote_up("QUOTING", now_ms - 1_000, now_ms, 30_000) == 1
    assert module.compute_quote_up("ON", now_ms - 1_000, now_ms, 30_000) == 1
    assert module.compute_quote_up("PAUSED", now_ms - 1_000, now_ms, 30_000) == 0
    assert module.compute_quote_up("QUOTING", now_ms - 30_001, now_ms, 30_000) == 0


def test_discover_strategy_contexts_filters_by_group_and_normalizes_venues(tmp_path) -> None:
    module = _load_exporter_module()

    cfg_dir = tmp_path / "configs"
    cfg_dir.mkdir(parents=True, exist_ok=True)
    (cfg_dir / "strategies.ini").write_text(
        "\n".join(
            [
                "[strategy:plumeusdt_bybit_perp_makerv3]",
                "strategy_groups = tokenmm",
                "exchange = bybit_linear",
                "base_asset = PLUME",
                "quote_asset = USDT",
                "",
                "[strategy:plumeusdt_okx_spot_makerv3]",
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
        "plumeusdt_bybit_perp_makerv3",
        "plumeusdt_okx_spot_makerv3",
    }
    assert contexts["plumeusdt_bybit_perp_makerv3"].venue == "bybit_linear"
    assert contexts["plumeusdt_bybit_perp_makerv3"].symbol == "PLUME/USDT"
    assert contexts["plumeusdt_okx_spot_makerv3"].venue == "okx_spot"
    assert contexts["plumeusdt_okx_spot_makerv3"].symbol == "PLUME/USDT"


def test_parse_strategy_context_supports_current_plumeusdt_strategy_ids() -> None:
    module = _load_exporter_module()

    context = module._parse_strategy_context("plumeusdt_bitget_perp_makerv3")

    assert context.token == "PLUME"
    assert context.venue == "bitget_perp"
    assert context.symbol == "PLUME/USDT"


def test_resolve_strategy_ids_falls_back_to_current_live_allowlist() -> None:
    module = _load_exporter_module()

    args = module._build_parser().parse_args(["--config-dir", "/tmp/tokenmm-missing-config-dir"])

    assert module._resolve_strategy_ids(args) == [
        "plumeusdt_bybit_perp_makerv3",
        "plumeusdt_bybit_spot_makerv3",
        "plumeusdt_okx_perp_makerv3",
        "plumeusdt_binance_perp_makerv3",
        "plumeusdt_binance_spot_makerv3",
        "plumeusdt_bitget_perp_makerv3",
        "plumeusdt_bitget_spot_makerv3",
    ]


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

    strategy_id = "plumeusdt_bybit_perp_makerv3"
    redis_client = FakeRedis()
    exporter = module.TokenMMMetricsExporter(
        redis_client=redis_client,
        env="prod",
        strategy_ids=[strategy_id],
    )
    now_ms = 1_700_000_000_000

    redis_client.set(
        f"flux:v1:state:{strategy_id}",
        _row_json(
            ts_ms=now_ms - 1_000,
            maker_leg={"exchange": "bybit_linear", "symbol": "PLUME_USDT"},
            maker_v3={
                "quote_snapshot": {
                    "ts_ms": now_ms - 1_000,
                    "mode": "ON",
                    "maker_top_bid": "100",
                    "maker_top_ask": "102",
                }
            },
            maker_orders={
                "bid": [{"px": "100", "rem_qty": "2", "status": "OPEN"}],
                "ask": [{"px": "102", "rem_qty": "1", "status": "LIVE"}],
            },
        ),
    )

    exporter.poll_quote_states(now_ms=now_ms)
    labels = {
        "env": "prod",
        "strategy_id": strategy_id,
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


def test_quote_state_poll_reads_flux_v1_state_and_params_contract() -> None:
    module = _load_exporter_module()

    strategy_id = "plumeusdt_bybit_spot_makerv3"
    redis_client = FakeRedis()
    exporter = module.TokenMMMetricsExporter(
        redis_client=redis_client,
        env="prod",
        strategy_ids=[strategy_id],
    )
    now_ms = 1_700_000_000_000

    redis_client.set(
        f"flux:v1:state:{strategy_id}",
        _row_json(
            ts_ms=now_ms - 1_000,
            effective_bot_on=True,
            maker_quote_status={
                "bid_open": 2,
                "ask_open": 3,
                "bid_depth": 5,
                "ask_depth": 5,
                "bid_blocked": 3,
                "ask_blocked": 2,
            },
            maker_v3={
                "quote_snapshot": {
                    "mode": "ON",
                    "maker_exchange": "bybit_spot",
                    "maker_symbol": "PLUME_USDT",
                    "maker_top_bid": "0.01320",
                    "maker_top_ask": "0.01330",
                }
            },
        ),
    )
    redis_client.hset(
        f"flux:v1:params:{strategy_id}",
        {
            "qty": "1000",
            "qty_unit": "base",
            "bid_edge1": "10",
            "ask_edge1": "10",
            "place_edge1": "2",
            "distance1": "2",
            "n_orders1": "5",
            "bid_edge2": "25",
            "ask_edge2": "25",
            "place_edge2": "2",
            "distance2": "5",
            "n_orders2": "0",
            "bid_edge3": "50",
            "ask_edge3": "50",
            "place_edge3": "2",
            "distance3": "5",
            "n_orders3": "0",
        },
    )

    exporter.poll_quote_states(now_ms=now_ms)
    labels = {
        "env": "prod",
        "strategy_id": strategy_id,
        "token": "PLUME",
        "venue": "bybit_spot",
        "symbol": "PLUME/USDT",
        "strategy_family": "maker_v3",
    }
    assert exporter.registry.get_sample_value("tokenmm_quote_up", labels) == 1.0
    assert exporter.registry.get_sample_value("tokenmm_quote_depth_usd_100bps", labels) == pytest.approx(
        66.3215,
    )
    assert exporter.registry.get_sample_value("tokenmm_quote_depth_usd_200bps", labels) == pytest.approx(
        66.3215,
    )


def test_quote_state_poll_merges_strategy_toml_defaults_with_partial_flux_params(
    tmp_path: Path,
) -> None:
    module = _load_exporter_module()

    strategy_id = "plumeusdt_bybit_spot_makerv3"
    strategy_config_dir = tmp_path / "strategies"
    strategy_config_dir.mkdir(parents=True, exist_ok=True)
    (strategy_config_dir / f"{strategy_id}.toml").write_text(
        "\n".join(
            [
                "[strategy]",
                'order_qty = "1000"',
                'qty = "1000"',
                'qty_unit = "base"',
                "bid_edge1 = 10.0",
                "ask_edge1 = 10.0",
                "place_edge1 = 2.0",
                "distance1 = 2.0",
                "n_orders1 = 5",
                "bid_edge2 = 25.0",
                "ask_edge2 = 25.0",
                "place_edge2 = 2.0",
                "distance2 = 5.0",
                "n_orders2 = 0",
                "bid_edge3 = 50.0",
                "ask_edge3 = 50.0",
                "place_edge3 = 2.0",
                "distance3 = 5.0",
                "n_orders3 = 0",
            ],
        ),
        encoding="utf-8",
    )

    redis_client = FakeRedis()
    exporter = module.TokenMMMetricsExporter(
        redis_client=redis_client,
        env="prod",
        strategy_ids=[strategy_id],
        strategy_param_defaults=module.discover_strategy_param_defaults(
            strategy_ids=[strategy_id],
            strategy_config_dir=str(strategy_config_dir),
        ),
    )
    now_ms = 1_700_000_000_000

    redis_client.set(
        f"flux:v1:state:{strategy_id}",
        _row_json(
            ts_ms=now_ms - 1_000,
            effective_bot_on=True,
            maker_quote_status={
                "bid_open": 2,
                "ask_open": 3,
                "bid_depth": 5,
                "ask_depth": 5,
                "bid_blocked": 3,
                "ask_blocked": 2,
            },
            maker_v3={
                "quote_snapshot": {
                    "mode": "ON",
                    "maker_exchange": "bybit_spot",
                    "maker_symbol": "PLUME_USDT",
                    "maker_top_bid": "0.01320",
                    "maker_top_ask": "0.01330",
                }
            },
        ),
    )
    redis_client.hset(
        f"flux:v1:params:{strategy_id}",
        {
            "bid_edge1": "12",
            "ask_edge1": "12",
        },
    )

    exporter.poll_quote_states(now_ms=now_ms)
    labels = {
        "env": "prod",
        "strategy_id": strategy_id,
        "token": "PLUME",
        "venue": "bybit_spot",
        "symbol": "PLUME/USDT",
        "strategy_family": "maker_v3",
    }
    assert exporter.registry.get_sample_value("tokenmm_quote_up", labels) == 1.0
    assert exporter.registry.get_sample_value("tokenmm_quote_depth_usd_100bps", labels) == pytest.approx(
        66.3242,
    )
    assert exporter.registry.get_sample_value("tokenmm_quote_depth_usd_200bps", labels) == pytest.approx(
        66.3242,
    )


def test_exporter_source_uses_flux_v1_state_contract() -> None:
    path = _repo_root() / "ops/scripts/exporters/tokenmm_metrics_exporter.py"
    assert path.exists(), "liquidity exporter script should exist"

    source = path.read_text(encoding="utf-8")

    assert "FluxRedisKeys" in source
    assert "self._state_key(strategy_id)" in source


def test_poll_quote_states_removes_stale_fallback_labels_after_context_update() -> None:
    module = _load_exporter_module()

    strategy_id = "plumeusdt_bybit_perp_makerv3"
    redis_client = FakeRedis()
    exporter = module.TokenMMMetricsExporter(
        redis_client=redis_client,
        env="prod",
        strategy_ids=[strategy_id],
    )
    now_ms = 1_700_000_000_000

    fallback_labels = {
        "env": "prod",
        "strategy_id": strategy_id,
        "token": "PLUME",
        "venue": "bybit_perp",
        "symbol": "PLUME/USDT",
        "strategy_family": "maker_v3",
    }
    live_labels = {
        "env": "prod",
        "strategy_id": strategy_id,
        "token": "PLUME",
        "venue": "bybit_linear",
        "symbol": "PLUME/USDT",
        "strategy_family": "maker_v3",
    }

    assert exporter.registry.get_sample_value("tokenmm_quote_up", fallback_labels) == 0.0
    assert exporter.registry.get_sample_value("tokenmm_quote_depth_usd_100bps", fallback_labels) == 0.0
    assert exporter.registry.get_sample_value("tokenmm_quote_depth_usd_200bps", fallback_labels) == 0.0

    redis_client.set(
        f"flux:v1:state:{strategy_id}",
        _row_json(
            ts_ms=now_ms - 1_000,
            maker_leg={"exchange": "bybit_linear", "symbol": "PLUME_USDT"},
            maker_v3={
                "quote_snapshot": {
                    "ts_ms": now_ms - 1_000,
                    "mode": "ON",
                    "maker_top_bid": "100",
                    "maker_top_ask": "102",
                }
            },
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

    healthy_strategy_id = "plumeusdt_okx_perp_makerv3"
    failed_strategy_id = "plumeusdt_bybit_perp_makerv3"
    now_ms = 1_700_000_000_000

    class FlakyRedis(FakeRedis):
        def get(self, key: str):
            if key == f"flux:v1:state:{failed_strategy_id}":
                raise RuntimeError("redis unavailable")
            return super().get(key)

    redis_client = FlakyRedis()
    redis_client.set(
        f"flux:v1:state:{healthy_strategy_id}",
        _row_json(
            ts_ms=now_ms - 1_000,
            maker_leg={"exchange": "okx_spot", "symbol": "PLUME_USDT"},
            maker_v3={
                "quote_snapshot": {
                    "ts_ms": now_ms - 1_000,
                    "mode": "ON",
                    "maker_top_bid": "100",
                    "maker_top_ask": "102",
                }
            },
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
        "strategy_id": healthy_strategy_id,
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


def test_default_redis_url_prefers_tokenmm_env_over_localhost(monkeypatch) -> None:
    module = _load_exporter_module()
    monkeypatch.delenv("REDIS_URL", raising=False)
    monkeypatch.setenv("TOKENMM_REDIS_HOST", "cache.example")
    monkeypatch.setenv("TOKENMM_REDIS_PORT", "6380")
    monkeypatch.setenv("TOKENMM_REDIS_DB", "7")
    monkeypatch.setenv("TOKENMM_REDIS_USERNAME", "default")
    monkeypatch.setenv("TOKENMM_REDIS_PASSWORD", "secret")
    monkeypatch.setenv("TOKENMM_REDIS_SSL", "true")

    assert module._default_redis_url() == "rediss://default:secret@cache.example:6380/7"


def test_quote_state_poll_emits_distinct_series_for_same_market_when_strategy_ids_differ() -> None:
    module = _load_exporter_module()

    strategy_ids = [
        "plumeusdt_bybit_spot_makerv3",
        "plumeusdt_bybit_spot_makerv3_alt",
    ]
    redis_client = FakeRedis()
    exporter = module.TokenMMMetricsExporter(
        redis_client=redis_client,
        env="prod",
        strategy_ids=strategy_ids,
    )
    now_ms = 1_700_000_000_000

    for strategy_id, top_bid, top_ask in (
        (strategy_ids[0], "0.01320", "0.01330"),
        (strategy_ids[1], "0.01310", "0.01340"),
    ):
        redis_client.set(
            f"flux:v1:state:{strategy_id}",
            _row_json(
                ts_ms=now_ms - 1_000,
                effective_bot_on=True,
                maker_quote_status={
                    "bid_open": 2,
                    "ask_open": 2,
                    "bid_depth": 2,
                    "ask_depth": 2,
                },
                maker_v3={
                    "quote_snapshot": {
                        "mode": "ON",
                        "maker_exchange": "bybit_spot",
                        "maker_symbol": "PLUME_USDT",
                        "maker_top_bid": top_bid,
                        "maker_top_ask": top_ask,
                    }
                },
            ),
        )
        redis_client.hset(
            f"flux:v1:params:{strategy_id}",
            {
                "qty": "1000",
                "qty_unit": "base",
                "bid_edge1": "10",
                "ask_edge1": "10",
                "place_edge1": "2",
                "distance1": "2",
                "n_orders1": "2",
                "n_orders2": "0",
                "n_orders3": "0",
            },
        )

    exporter.poll_quote_states(now_ms=now_ms)

    first_labels = {
        "env": "prod",
        "strategy_id": strategy_ids[0],
        "token": "PLUME",
        "venue": "bybit_spot",
        "symbol": "PLUME/USDT",
        "strategy_family": "maker_v3",
    }
    second_labels = {
        "env": "prod",
        "strategy_id": strategy_ids[1],
        "token": "PLUME",
        "venue": "bybit_spot",
        "symbol": "PLUME/USDT",
        "strategy_family": "maker_v3",
    }

    assert exporter.registry.get_sample_value("tokenmm_quote_up", first_labels) == 1.0
    assert exporter.registry.get_sample_value("tokenmm_quote_up", second_labels) == 1.0
    assert first_labels != second_labels
