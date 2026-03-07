from __future__ import annotations

import sys
from pathlib import Path
from types import SimpleNamespace

import pytest


try:
    import redis as _redis  # noqa: F401
except ModuleNotFoundError:  # pragma: no cover - local test harness fallback
    sys.modules["redis"] = SimpleNamespace(Redis=None)

from flux.runners.tokenmm import run_node
from nautilus_trader.model.identifiers import InstrumentId


class _DummyStrategy:
    def __init__(self) -> None:
        self.params_manager_factory = None
        self.portfolio_inventory_feed: dict[str, object] | None = None

    def set_params_manager_factory(self, factory) -> None:
        self.params_manager_factory = factory

    def configure_portfolio_inventory_feed(self, **kwargs) -> None:
        self.portfolio_inventory_feed = kwargs


def test_attach_runtime_params_manager_wires_redis_backed_factory(monkeypatch) -> None:
    strategy = _DummyStrategy()
    redis_call: dict[str, object] = {}
    factory_call: dict[str, object] = {}
    redis_client = object()
    sentinel_factory = object()

    def _fake_redis(**kwargs):
        redis_call.update(kwargs)
        return redis_client

    def _fake_params_manager_factory(**kwargs):
        factory_call.update(kwargs)
        return sentinel_factory

    monkeypatch.setattr(run_node.redis, "Redis", _fake_redis)
    monkeypatch.setattr(
        run_node.runtime_params_mod,
        "params_manager_factory",
        _fake_params_manager_factory,
    )

    run_node._attach_runtime_params_manager(
        strategy=strategy,
        redis_cfg={
            "host": "127.0.0.10",
            "port": 6381,
            "db": 4,
            "username": "alice",
            "password": "secret",
            "connect_timeout_secs": 7.5,
            "read_timeout_secs": 8.5,
        },
        namespace="fluxx",
        schema_version="v2",
    )

    assert redis_call == {
        "host": "127.0.0.10",
        "port": 6381,
        "db": 4,
        "username": "alice",
        "password": "secret",
        "ssl": False,
        "socket_connect_timeout": 7.5,
        "socket_timeout": 8.5,
        "decode_responses": False,
    }
    assert factory_call == {
        "redis_client": redis_client,
        "namespace": "fluxx",
        "schema_version": "v2",
    }
    assert strategy.params_manager_factory is sentinel_factory


def test_attach_portfolio_inventory_feed_wires_shared_portfolio_reader(monkeypatch) -> None:
    strategy = _DummyStrategy()
    redis_call: dict[str, object] = {}
    redis_client = object()

    def _fake_redis(**kwargs):
        redis_call.update(kwargs)
        return redis_client

    monkeypatch.setattr(run_node.redis, "Redis", _fake_redis)

    run_node._attach_portfolio_inventory_feed(
        strategy=strategy,
        config={"portfolio": {"portfolio_id": "tokenmm", "inventory_stale_after_ms": 2500}},
        redis_cfg={"host": "127.0.0.10", "port": 6381, "db": 4},
        namespace="fluxx",
        schema_version="v2",
    )

    assert redis_call["host"] == "127.0.0.10"
    assert strategy.portfolio_inventory_feed == {
        "redis_client": redis_client,
        "portfolio_id": "tokenmm",
        "namespace": "fluxx",
        "schema_version": "v2",
        "stale_after_ms": 2500,
    }


def test_build_node_defaults_live_message_bus_streams_to_autotrim(monkeypatch) -> None:
    captured: dict[str, object] = {}

    class _CapturedNode:
        def __init__(self, config) -> None:
            captured["config"] = config
            self.trader = SimpleNamespace(add_strategy=lambda _strategy: None)

        def add_data_client_factory(self, _venue, _factory) -> None:
            return None

        def add_exec_client_factory(self, _venue, _factory) -> None:
            return None

        def build(self) -> None:
            return None

    class _CapturedStrategy:
        def __init__(self, *, config) -> None:
            self.config = config

        def set_params_manager_factory(self, _factory) -> None:
            return None

        def configure_portfolio_inventory_feed(self, **_kwargs) -> None:
            return None

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    monkeypatch.setattr(run_node, "MakerV3Strategy", _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("PLUMEUSDT-PERP.BYBIT"),
            reference_instrument_id=InstrumentId.from_str("PLUMEUSDT.BINANCE_SPOT"),
            data_clients={},
            exec_clients={},
            data_factories={},
            exec_factories={},
        ),
    )
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)

    node = run_node.build_node(
        {
            "flux": {"namespace": "flux", "schema_version": "v1"},
            "identity": {
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "external_strategy_id": "plumeusdt_bybit_perp_makerv3",
                "trader_id": "TOKENMM-LIVE-BYBIT-PERP",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False},
            "strategy": {"strategy_id": "plumeusdt_bybit_perp_makerv3", "order_qty": "1000"},
        },
        mode="live",
        force_enable_execution=False,
    )

    assert node is not None
    config = captured["config"]
    assert config.message_bus is not None
    assert config.message_bus.autotrim_mins == 30


def test_build_node_honors_explicit_message_bus_autotrim_override(monkeypatch) -> None:
    captured: dict[str, object] = {}

    class _CapturedNode:
        def __init__(self, config) -> None:
            captured["config"] = config
            self.trader = SimpleNamespace(add_strategy=lambda _strategy: None)

        def add_data_client_factory(self, _venue, _factory) -> None:
            return None

        def add_exec_client_factory(self, _venue, _factory) -> None:
            return None

        def build(self) -> None:
            return None

    class _CapturedStrategy:
        def __init__(self, *, config) -> None:
            self.config = config

        def set_params_manager_factory(self, _factory) -> None:
            return None

        def configure_portfolio_inventory_feed(self, **_kwargs) -> None:
            return None

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    monkeypatch.setattr(run_node, "MakerV3Strategy", _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("PLUMEUSDT-PERP.BYBIT"),
            reference_instrument_id=InstrumentId.from_str("PLUMEUSDT.BINANCE_SPOT"),
            data_clients={},
            exec_clients={},
            data_factories={},
            exec_factories={},
        ),
    )
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)

    run_node.build_node(
        {
            "flux": {"namespace": "flux", "schema_version": "v1"},
            "identity": {
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "external_strategy_id": "plumeusdt_bybit_perp_makerv3",
                "trader_id": "TOKENMM-LIVE-BYBIT-PERP",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False, "message_bus_autotrim_mins": 12},
            "strategy": {"strategy_id": "plumeusdt_bybit_perp_makerv3", "order_qty": "1000"},
        },
        mode="live",
        force_enable_execution=False,
    )

    config = captured["config"]
    assert config.message_bus is not None
    assert config.message_bus.autotrim_mins == 12


def test_resolve_reconciliation_settings_enforces_live_minimum_startup_delay() -> None:
    lookback, startup_delay = run_node._resolve_reconciliation_settings(
        mode="live",
        node_cfg={
            "exec_reconciliation_lookback_mins": -5,
            "exec_reconciliation_startup_delay_secs": 1.0,
        },
    )

    assert lookback == 0
    assert startup_delay == 10.0


def test_resolve_reconciliation_settings_keeps_dev_values_in_paper_mode() -> None:
    lookback, startup_delay = run_node._resolve_reconciliation_settings(
        mode="paper",
        node_cfg={
            "exec_reconciliation_lookback_mins": 5,
            "exec_reconciliation_startup_delay_secs": 1.0,
        },
    )

    assert lookback == 5
    assert startup_delay == 1.0


def test_resolve_reconciliation_settings_keeps_positive_live_lookback() -> None:
    lookback, startup_delay = run_node._resolve_reconciliation_settings(
        mode="live",
        node_cfg={
            "exec_reconciliation_lookback_mins": 15,
            "exec_reconciliation_startup_delay_secs": 12.0,
        },
    )

    assert lookback == 15
    assert startup_delay == 12.0


def test_redis_database_config_uses_redis_section_values() -> None:
    database = run_node._redis_database_config(
        {
            "host": "127.0.0.10",
            "port": 6381,
            "username": "alice",
            "password": "secret",
            "ssl": True,
        },
    )

    assert database.type == "redis"
    assert database.host == "127.0.0.10"
    assert database.port == 6381
    assert database.username == "alice"
    assert database.password == "secret"
    assert database.ssl is True


def test_client_order_id_config_disables_hyphens_for_okx() -> None:
    instrument_id = InstrumentId.from_str("PLUME-USDT-SWAP.OKX")

    assert run_node._client_order_id_config(instrument_id) == {
        "use_hyphens_in_client_order_ids": False,
    }


def test_client_order_id_config_leaves_non_okx_unchanged() -> None:
    instrument_id = InstrumentId.from_str("PLUMEUSDT-PERP.BYBIT")

    assert run_node._client_order_id_config(instrument_id) == {}


def test_resolve_execution_filter_settings_defaults_disabled() -> None:
    assert run_node._resolve_execution_filter_settings({}) == (False, False)


def test_resolve_execution_filter_settings_honors_shared_account_flags() -> None:
    assert run_node._resolve_execution_filter_settings(
        {
            "filter_unclaimed_external_orders": True,
            "filter_position_reports": True,
        },
    ) == (True, True)


def test_parse_args_requires_explicit_config(monkeypatch) -> None:
    monkeypatch.setattr("sys.argv", ["run_node.py"])

    with pytest.raises(SystemExit, match="2"):
        run_node._parse_args()


def test_merge_shared_tables_inherits_missing_redis_table() -> None:
    merged = run_node._merge_shared_tables(
        config={
            "identity": {"strategy_id": "strategy_a"},
            "node": {"enable_execution": False},
        },
        shared_config={
            "redis": {"host": "127.0.0.1", "port": 6380, "db": 0},
            "api": {"host": "127.0.0.1"},
        },
        table_names=("redis",),
    )

    assert merged["redis"] == {"host": "127.0.0.1", "port": 6380, "db": 0}
    assert "api" not in merged


def test_merge_shared_tables_can_inherit_portfolio_table() -> None:
    merged = run_node._merge_shared_tables(
        config={
            "identity": {"strategy_id": "strategy_a"},
        },
        shared_config={
            "portfolio": {"portfolio_id": "tokenmm"},
        },
        table_names=("portfolio",),
    )

    assert merged["portfolio"] == {"portfolio_id": "tokenmm"}


def test_merge_shared_tables_keeps_node_specific_redis_override() -> None:
    merged = run_node._merge_shared_tables(
        config={
            "redis": {"host": "127.0.0.2", "port": 6380, "db": 1},
        },
        shared_config={
            "redis": {"host": "127.0.0.1", "port": 6380, "db": 0},
        },
        table_names=("redis",),
    )

    assert merged["redis"] == {"host": "127.0.0.2", "port": 6380, "db": 1}


def test_load_runtime_config_merges_shared_redis_from_top_level_file(tmp_path: Path) -> None:
    strategy_path = tmp_path / "strategy.toml"
    shared_path = tmp_path / "shared.toml"
    strategy_path.write_text(
        """
[flux]
mode = "paper"

[identity]
strategy_id = "strategy_a"

[node]
enable_execution = false
""".strip(),
        encoding="utf-8",
    )
    shared_path.write_text(
        """
[redis]
host = "127.0.0.1"
port = 6380
db = 0
""".strip(),
        encoding="utf-8",
    )

    merged = run_node._load_runtime_config(strategy_path, shared_config_path=shared_path)

    assert merged["redis"]["host"] == "127.0.0.1"
    assert merged["redis"]["db"] == 0


def test_load_runtime_config_merges_shared_telemetry_shipper_table(tmp_path: Path) -> None:
    strategy_path = tmp_path / "strategy.toml"
    shared_path = tmp_path / "shared.toml"
    strategy_path.write_text(
        """
[flux]
mode = "paper"

[identity]
strategy_id = "strategy_a"

[node]
enable_execution = false
""".strip(),
        encoding="utf-8",
    )
    shared_path.write_text(
        """
[telemetry_shipper]
enabled = true
enable_local_persistence = true
source_profile = "tokenmm"
balance_snapshots_db_path = "/var/lib/nautilus/telemetry/tokenmm/balance_snapshots.sqlite"
fills_db_path = "/var/lib/nautilus/telemetry/tokenmm/fills.sqlite"
orders_db_path = "/var/lib/nautilus/telemetry/tokenmm/orders.sqlite"
quote_cycles_db_path = "/var/lib/nautilus/telemetry/tokenmm/quote_cycles.sqlite"
state_db_path = "/var/lib/nautilus/telemetry/tokenmm/shipper_state.sqlite"
poll_interval_ms = 1000
max_batch_size = 500
prune_retention_hours = 168
""".strip(),
        encoding="utf-8",
    )

    merged = run_node._load_runtime_config(strategy_path, shared_config_path=shared_path)

    assert merged["telemetry_shipper"]["source_profile"] == "tokenmm"
    assert (
        merged["telemetry_shipper"]["balance_snapshots_db_path"]
        == "/var/lib/nautilus/telemetry/tokenmm/balance_snapshots.sqlite"
    )
    assert (
        merged["telemetry_shipper"]["fills_db_path"]
        == "/var/lib/nautilus/telemetry/tokenmm/fills.sqlite"
    )


def test_load_runtime_config_applies_redis_env_overrides(tmp_path: Path, monkeypatch) -> None:
    strategy_path = tmp_path / "strategy.toml"
    strategy_path.write_text(
        """
[flux]
mode = "paper"

[identity]
strategy_id = "strategy_a"

[redis]
host = "127.0.0.1"
port = 6380
db = 0
ssl = false

[node]
enable_execution = false
""".strip(),
        encoding="utf-8",
    )
    monkeypatch.setenv(
        "TOKENMM_REDIS_HOST",
        "master.maker-v2-client-redis-prod.wapqos.apse1.cache.amazonaws.com",
    )
    monkeypatch.setenv("TOKENMM_REDIS_PORT", "6379")
    monkeypatch.setenv("TOKENMM_REDIS_USERNAME", "default")
    monkeypatch.setenv("TOKENMM_REDIS_PASSWORD", "secret")
    monkeypatch.setenv("TOKENMM_REDIS_SSL", "true")

    merged = run_node._load_runtime_config(strategy_path)

    assert (
        merged["redis"]["host"]
        == "master.maker-v2-client-redis-prod.wapqos.apse1.cache.amazonaws.com"
    )
    assert merged["redis"]["port"] == 6379
    assert merged["redis"]["username"] == "default"
    assert merged["redis"]["password"] == "secret"
    assert merged["redis"]["ssl"] is True


def test_parse_args_accepts_optional_shared_config(monkeypatch, tmp_path: Path) -> None:
    config_path = tmp_path / "strategy.toml"
    shared_path = tmp_path / "shared.toml"
    monkeypatch.setattr(
        "sys.argv",
        [
            "run_node.py",
            "--config",
            str(config_path),
            "--shared-config",
            str(shared_path),
        ],
    )

    args = run_node._parse_args()

    assert args.config == config_path
    assert args.shared_config == shared_path


def test_build_node_adds_local_telemetry_persistence_actors_when_enabled(monkeypatch) -> None:
    captured: dict[str, object] = {}

    class _CapturedNode:
        def __init__(self, config) -> None:
            captured["config"] = config
            self.trader = SimpleNamespace(add_strategy=lambda _strategy: None)

        def add_data_client_factory(self, _venue, _factory) -> None:
            return None

        def add_exec_client_factory(self, _venue, _factory) -> None:
            return None

        def build(self) -> None:
            return None

    class _CapturedStrategy:
        def __init__(self, *, config) -> None:
            self.config = config

        def set_params_manager_factory(self, _factory) -> None:
            return None

        def configure_portfolio_inventory_feed(self, **_kwargs) -> None:
            return None

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    monkeypatch.setattr(run_node, "MakerV3Strategy", _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("PLUMEUSDT-PERP.BYBIT"),
            reference_instrument_id=InstrumentId.from_str("PLUMEUSDT.BINANCE_SPOT"),
            data_clients={},
            exec_clients={},
            data_factories={},
            exec_factories={},
        ),
    )
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)

    run_node.build_node(
        {
            "flux": {"namespace": "flux", "schema_version": "v1"},
            "identity": {
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "external_strategy_id": "plumeusdt_bybit_perp_makerv3",
                "trader_id": "TOKENMM-LIVE-BYBIT-PERP",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False},
            "strategy": {"strategy_id": "plumeusdt_bybit_perp_makerv3", "order_qty": "1000"},
            "telemetry_shipper": {
                "enabled": True,
                "enable_local_persistence": True,
                "source_profile": "tokenmm",
                "balance_snapshots_db_path": "/var/lib/nautilus/telemetry/tokenmm/balance_snapshots.sqlite",
                "fills_db_path": "/var/lib/nautilus/telemetry/tokenmm/fills.sqlite",
                "orders_db_path": "/var/lib/nautilus/telemetry/tokenmm/orders.sqlite",
                "quote_cycles_db_path": "/var/lib/nautilus/telemetry/tokenmm/quote_cycles.sqlite",
                "state_db_path": "/var/lib/nautilus/telemetry/tokenmm/shipper_state.sqlite",
                "poll_interval_ms": 1000,
                "max_batch_size": 500,
                "prune_retention_hours": 168,
            },
        },
        mode="live",
        force_enable_execution=False,
    )

    config = captured["config"]
    actors = config.actors
    assert len(actors) == 4
    assert {
        actor.actor_path
        for actor in actors
    } == {
        "nautilus_trader.flux.persistence.balance_snapshots.actor.FluxBalanceSnapshotPersistenceActor",
        "nautilus_trader.persistence.fills.actor.ExecutionFillPersistenceActor",
        "nautilus_trader.persistence.orders.actor.OrderActionPersistenceActor",
        "nautilus_trader.flux.persistence.quote_cycles.actor.QuoteCyclePersistenceActor",
    }


def test_strategy_startup_lock_prevents_duplicate_flux_strategy_ids(tmp_path: Path) -> None:
    config = {
        "identity": {"strategy_id": "plumeusdt_bybit_perp_makerv3"},
    }

    with (
        run_node._strategy_startup_lock(config, lock_dir=tmp_path),
        pytest.raises(
            RuntimeError,
            match="already running",
        ),
        run_node._strategy_startup_lock(config, lock_dir=tmp_path),
    ):
        pass


def test_strategy_startup_lock_releases_after_context_exit(tmp_path: Path) -> None:
    config = {
        "identity": {"strategy_id": "plumeusdt_bybit_perp_makerv3"},
    }

    context = run_node._strategy_startup_lock(config, lock_dir=tmp_path)
    with context:
        pass

    with run_node._strategy_startup_lock(config, lock_dir=tmp_path):
        pass
