from __future__ import annotations

from types import SimpleNamespace
from pathlib import Path

import pytest

from flux.runners.shared.bootstrap import strategy_startup_lock
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.runners.tokenmm import run_node
from nautilus_trader.adapters.bybit import BYBIT
from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.node import TradingNodeFatalError
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money


class _DummyStrategy:
    def __init__(self) -> None:
        self.params_manager_factory = None
        self.portfolio_inventory_feed: dict[str, object] | None = None

    def set_params_manager_factory(self, factory) -> None:
        self.params_manager_factory = factory

    def configure_portfolio_inventory_feed(self, **kwargs) -> None:
        self.portfolio_inventory_feed = kwargs


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _install_strategy_spec(
    monkeypatch,
    strategy_cls: type[object],
    *,
    config_cls: type[object] | None = None,
) -> None:
    monkeypatch.setattr(
        run_node,
        "get_strategy_spec",
        lambda name: (
            SimpleNamespace(
                name=name,
                strategy_cls=strategy_cls,
                config_cls=config_cls or run_node.MakerV3StrategyConfig,
                param_set="makerv3",
                strategy_family="maker_v3",
                strategy_version="v3",
            )
        ),
        raising=False,
    )


def test_tokenmm_startup_lock_uses_descriptor_specific_lock_dir(tmp_path: Path) -> None:
    config = {"identity": {"strategy_id": "plumeusdt_bybit_perp_makerv3"}}

    with strategy_startup_lock(
        config,
        descriptor=get_strategy_set_descriptor("tokenmm"),
        repo_root=tmp_path,
    ):
        assert (
            tmp_path / ".run" / "tokenmm-strategy-locks" / "plumeusdt_bybit_perp_makerv3.lock"
        ).exists()


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
        "allow_partial_global_risk": False,
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
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
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


def test_build_node_resolves_strategy_via_registry(monkeypatch) -> None:
    captured: dict[str, object] = {}
    registry_calls: list[str] = []

    class _CapturedNode:
        def __init__(self, config) -> None:
            captured["config"] = config
            self.trader = SimpleNamespace(
                add_strategy=lambda strategy: captured.setdefault("strategy", strategy),
            )

        def add_data_client_factory(self, _venue, _factory) -> None:
            return None

        def add_exec_client_factory(self, _venue, _factory) -> None:
            return None

        def build(self) -> None:
            return None

    class _CapturedStrategyConfig:
        def __init__(self, **kwargs) -> None:
            self.__dict__.update(kwargs)

    class _CapturedStrategy:
        def __init__(self, *, config) -> None:
            self.config = config

        def set_params_manager_factory(self, _factory) -> None:
            return None

        def configure_portfolio_inventory_feed(self, **_kwargs) -> None:
            return None

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    monkeypatch.setattr(
        run_node,
        "get_strategy_spec",
        lambda name: (
            registry_calls.append(name)
            or SimpleNamespace(
                strategy_cls=_CapturedStrategy,
                config_cls=_CapturedStrategyConfig,
                param_set="makerv3",
                strategy_family="maker_v3",
                strategy_version="v3",
            )
        ),
        raising=False,
    )
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
        },
        mode="paper",
        force_enable_execution=False,
    )

    assert registry_calls == ["makerv3"]
    assert isinstance(captured["strategy"], _CapturedStrategy)
    assert isinstance(captured["strategy"].config, _CapturedStrategyConfig)


def test_build_node_warns_when_qty_unit_missing_and_defaults_to_venue(
    monkeypatch,
    caplog,
) -> None:
    captured: dict[str, object] = {}

    class _CapturedNode:
        def __init__(self, config) -> None:
            captured["config"] = config
            self.trader = SimpleNamespace(
                add_strategy=lambda strategy: captured.setdefault("strategy", strategy),
            )

        def add_data_client_factory(self, _venue, _factory) -> None:
            return None

        def add_exec_client_factory(self, _venue, _factory) -> None:
            return None

        def build(self) -> None:
            return None

    class _CapturedStrategyConfig:
        def __init__(self, **kwargs) -> None:
            self.__dict__.update(kwargs)

    class _CapturedStrategy:
        def __init__(self, *, config) -> None:
            self.config = config

        def set_params_manager_factory(self, _factory) -> None:
            return None

        def configure_portfolio_inventory_feed(self, **_kwargs) -> None:
            return None

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    monkeypatch.setattr(
        run_node,
        "get_strategy_spec",
        lambda _name: SimpleNamespace(
            strategy_cls=_CapturedStrategy,
            config_cls=_CapturedStrategyConfig,
            param_set="makerv3",
            strategy_family="maker_v3",
            strategy_version="v3",
        ),
        raising=False,
    )
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

    with caplog.at_level("WARNING", logger=run_node.__name__):
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
            },
            mode="paper",
            force_enable_execution=False,
        )

    assert captured["strategy"].config.qty_unit == "venue"
    assert "qty_unit missing" in caplog.text
    assert "defaulting to 'venue'" in caplog.text


def test_build_node_rejects_invalid_qty_unit(monkeypatch) -> None:
    class _CapturedNode:
        def __init__(self, config=None, **_kwargs) -> None:
            self.trader = SimpleNamespace(add_strategy=lambda _strategy: None)

        def add_data_client_factory(self, _venue, _factory) -> None:
            return None

        def add_exec_client_factory(self, _venue, _factory) -> None:
            return None

        def build(self) -> None:
            return None

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
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

    with pytest.raises(ValueError, match="Unsupported qty_unit"):
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
                "strategy": {
                    "strategy_id": "plumeusdt_bybit_perp_makerv3",
                    "order_qty": "1000",
                    "qty_unit": "contracts",
                },
            },
            mode="paper",
            force_enable_execution=False,
        )


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
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
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


def test_build_node_explicit_enable_execution_false_overrides_force_flag(monkeypatch) -> None:
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

    def _resolve_strategy_venues(**kwargs):
        captured["enable_execution"] = kwargs["enable_execution"]
        return SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("PLUMEUSDT-PERP.BYBIT"),
            reference_instrument_id=InstrumentId.from_str("PLUMEUSDT.BINANCE_SPOT"),
            data_clients={},
            exec_clients={},
            data_factories={},
            exec_factories={},
        )

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
    monkeypatch.setattr(run_node, "resolve_strategy_venues", _resolve_strategy_venues)
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
        },
        mode="live",
        force_enable_execution=True,
    )

    assert captured["enable_execution"] is False


def test_build_node_wires_exec_engine_purge_settings(monkeypatch) -> None:
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
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
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
            "node": {
                "enable_execution": False,
                "purge_closed_orders_interval_mins": 10,
                "purge_closed_orders_buffer_mins": 60,
                "purge_closed_positions_interval_mins": 12,
                "purge_closed_positions_buffer_mins": 90,
                "purge_account_events_interval_mins": 15,
                "purge_account_events_lookback_mins": 120,
                "purge_from_database": True,
            },
            "strategy": {"strategy_id": "plumeusdt_bybit_perp_makerv3", "order_qty": "1000"},
        },
        mode="live",
        force_enable_execution=False,
    )

    exec_engine = captured["config"].exec_engine
    assert exec_engine.purge_closed_orders_interval_mins == 10
    assert exec_engine.purge_closed_orders_buffer_mins == 60
    assert exec_engine.purge_closed_positions_interval_mins == 12
    assert exec_engine.purge_closed_positions_buffer_mins == 90
    assert exec_engine.purge_account_events_interval_mins == 15
    assert exec_engine.purge_account_events_lookback_mins == 120
    assert exec_engine.purge_from_database is True


def test_build_node_wires_exec_engine_generate_missing_orders_setting(monkeypatch) -> None:
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
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
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
            "node": {
                "enable_execution": False,
                "exec_generate_missing_orders": False,
            },
            "strategy": {"strategy_id": "plumeusdt_bybit_perp_makerv3", "order_qty": "1000"},
        },
        mode="live",
        force_enable_execution=False,
    )

    exec_engine = captured["config"].exec_engine
    assert exec_engine.generate_missing_orders is False


def test_build_node_defaults_non_flattening_manage_stop_graceful_shutdown_and_allowed_submit_instrument_ids(
    monkeypatch,
) -> None:
    captured: dict[str, object] = {}

    class _CapturedNode:
        def __init__(self, config) -> None:
            captured["node_config"] = config
            self.trader = SimpleNamespace(
                add_strategy=lambda strategy: captured.setdefault("strategy", strategy),
            )

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

    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT")

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=maker_instrument_id,
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
            "node": {"enable_execution": True},
            "strategy": {"strategy_id": "plumeusdt_bybit_perp_makerv3", "order_qty": "1000"},
        },
        mode="live",
        force_enable_execution=False,
    )

    node_config = captured["node_config"]
    strategy = captured["strategy"]

    assert node_config.data_engine.graceful_shutdown_on_exception is True
    assert node_config.risk_engine.graceful_shutdown_on_exception is True
    assert node_config.exec_engine.graceful_shutdown_on_exception is True
    assert strategy.config.manage_stop is False
    assert strategy.config.cancel_all_instrument_orders is False
    assert strategy.config.allowed_submit_instrument_ids == [maker_instrument_id]


def test_build_node_honors_explicit_manage_stop_and_graceful_shutdown_overrides(monkeypatch) -> None:
    captured: dict[str, object] = {}

    class _CapturedNode:
        def __init__(self, config) -> None:
            captured["node_config"] = config
            self.trader = SimpleNamespace(
                add_strategy=lambda strategy: captured.setdefault("strategy", strategy),
            )

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
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT"),
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
            "node": {
                "enable_execution": True,
                "graceful_shutdown_on_exception": False,
            },
            "strategy": {
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "order_qty": "1000",
                "manage_stop": False,
            },
        },
        mode="live",
        force_enable_execution=False,
    )

    node_config = captured["node_config"]
    strategy = captured["strategy"]

    assert node_config.data_engine.graceful_shutdown_on_exception is False
    assert node_config.risk_engine.graceful_shutdown_on_exception is False
    assert node_config.exec_engine.graceful_shutdown_on_exception is False
    assert strategy.config.manage_stop is False


def test_build_node_honors_explicit_cancel_all_instrument_orders_override(monkeypatch) -> None:
    captured: dict[str, object] = {}

    class _CapturedNode:
        def __init__(self, config) -> None:
            captured["node_config"] = config
            self.trader = SimpleNamespace(
                add_strategy=lambda strategy: captured.setdefault("strategy", strategy),
            )

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
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT"),
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
            "node": {"enable_execution": True},
            "strategy": {
                "strategy_id": "plumeusdt_bybit_perp_makerv3",
                "order_qty": "1000",
                "cancel_all_instrument_orders": True,
            },
        },
        mode="live",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]

    assert strategy.config.cancel_all_instrument_orders is True


def test_build_node_registers_cash_borrowing_before_trading_node(monkeypatch) -> None:
    captured: dict[str, object] = {}
    issuer = str(BYBIT)
    AccountFactory.deregister_cash_borrowing(issuer)

    class _CapturedNode:
        def __init__(self, config) -> None:
            initial_event = AccountState(
                account_id=AccountId("BYBIT-UNIFIED"),
                account_type=AccountType.CASH,
                base_currency=USDT,
                reported=True,
                balances=[
                    AccountBalance(
                        Money(1000, USDT),
                        Money(0, USDT),
                        Money(1000, USDT),
                    ),
                ],
                margins=[],
                info={},
                event_id=UUID4(),
                ts_event=1,
                ts_init=1,
            )
            borrowed_event = AccountState(
                account_id=AccountId("BYBIT-UNIFIED"),
                account_type=AccountType.CASH,
                base_currency=USDT,
                reported=True,
                balances=[
                    AccountBalance(
                        Money(-5, USDT),
                        Money(0, USDT),
                        Money(-5, USDT),
                    ),
                ],
                margins=[],
                info={},
                event_id=UUID4(),
                ts_event=2,
                ts_init=2,
            )
            account = AccountFactory.create(initial_event)
            account.apply(borrowed_event)
            captured["allow_borrowing"] = account.allow_borrowing
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
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("PLUMEUSDT-SPOT.BYBIT"),
            reference_instrument_id=InstrumentId.from_str("PLUMEUSDT.BINANCE_SPOT"),
            data_clients={},
            exec_clients={BYBIT: SimpleNamespace(allow_cash_borrowing=True)},
            data_factories={},
            exec_factories={},
        ),
    )
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)

    try:
        run_node.build_node(
            {
                "flux": {"namespace": "flux", "schema_version": "v1"},
                "identity": {
                    "strategy_id": "plumeusdt_bybit_spot_makerv3",
                    "external_strategy_id": "plumeusdt_bybit_spot_makerv3",
                    "trader_id": "TOKENMM-LIVE-BYBIT-SPOT",
                },
                "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
                "node": {"enable_execution": True},
                "strategy": {"strategy_id": "plumeusdt_bybit_spot_makerv3", "order_qty": "1000"},
            },
            mode="live",
            force_enable_execution=False,
        )
        assert captured["allow_borrowing"] is True
    finally:
        AccountFactory.deregister_cash_borrowing(issuer)


def test_build_node_registers_cash_borrowing_for_binance_spot_before_trading_node(
    monkeypatch,
) -> None:
    captured: dict[str, object] = {}
    execution_instrument_id = InstrumentId.from_str("PLUMEUSDT.BINANCE_SPOT")
    issuer = str(execution_instrument_id.venue)
    AccountFactory.deregister_cash_borrowing(issuer)

    class _CapturedNode:
        def __init__(self, config) -> None:
            initial_event = AccountState(
                account_id=AccountId("BINANCE_SPOT-MARGIN-master"),
                account_type=AccountType.CASH,
                base_currency=USDT,
                reported=True,
                balances=[
                    AccountBalance(
                        Money(1000, USDT),
                        Money(0, USDT),
                        Money(1000, USDT),
                    ),
                ],
                margins=[],
                info={},
                event_id=UUID4(),
                ts_event=1,
                ts_init=1,
            )
            borrowed_event = AccountState(
                account_id=AccountId("BINANCE_SPOT-MARGIN-master"),
                account_type=AccountType.CASH,
                base_currency=USDT,
                reported=True,
                balances=[
                    AccountBalance(
                        Money(-5, USDT),
                        Money(0, USDT),
                        Money(-5, USDT),
                    ),
                ],
                margins=[],
                info={},
                event_id=UUID4(),
                ts_event=2,
                ts_init=2,
            )
            account = AccountFactory.create(initial_event)
            account.apply(borrowed_event)
            captured["allow_borrowing"] = account.allow_borrowing
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
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=execution_instrument_id,
            reference_instrument_id=execution_instrument_id,
            data_clients={},
            exec_clients={execution_instrument_id.venue: SimpleNamespace(allow_cash_borrowing=True)},
            data_factories={},
            exec_factories={},
        ),
    )
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)

    try:
        run_node.build_node(
            {
                "flux": {"namespace": "flux", "schema_version": "v1"},
                "identity": {
                    "strategy_id": "plumeusdt_binance_spot_makerv3",
                    "external_strategy_id": "plumeusdt_binance_spot_makerv3",
                    "trader_id": "TOKENMM-LIVE-BINANCE-SPOT",
                },
                "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
                "node": {"enable_execution": True},
                "strategy": {"strategy_id": "plumeusdt_binance_spot_makerv3", "order_qty": "1000"},
            },
            mode="live",
            force_enable_execution=False,
        )
        assert captured["allow_borrowing"] is True
    finally:
        AccountFactory.deregister_cash_borrowing(issuer)


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


def test_build_node_rejects_live_execution_when_startup_reconciliation_is_disabled(
    monkeypatch,
) -> None:
    class _CapturedNode:
        def __init__(self, config) -> None:
            self.config = config
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
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
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

    with pytest.raises(ValueError, match="exec_reconciliation=true"):
        run_node.build_node(
            {
                "flux": {"namespace": "flux", "schema_version": "v1"},
                "identity": {
                    "strategy_id": "plumeusdt_bybit_perp_makerv3",
                    "external_strategy_id": "plumeusdt_bybit_perp_makerv3",
                    "trader_id": "TOKENMM-LIVE-BYBIT-PERP",
                },
                "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
                "node": {
                    "enable_execution": True,
                    "exec_reconciliation": False,
                },
                "strategy": {"strategy_id": "plumeusdt_bybit_perp_makerv3", "order_qty": "1000"},
            },
            mode="live",
            force_enable_execution=False,
        )


def test_build_node_rejects_live_execution_when_position_report_filtering_is_enabled(
    monkeypatch,
) -> None:
    class _CapturedNode:
        def __init__(self, config) -> None:
            self.config = config
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
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
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

    with pytest.raises(ValueError, match="filter_position_reports=false"):
        run_node.build_node(
            {
                "flux": {"namespace": "flux", "schema_version": "v1"},
                "identity": {
                    "strategy_id": "plumeusdt_bybit_perp_makerv3",
                    "external_strategy_id": "plumeusdt_bybit_perp_makerv3",
                    "trader_id": "TOKENMM-LIVE-BYBIT-PERP",
                },
                "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
                "node": {
                    "enable_execution": True,
                    "filter_position_reports": True,
                },
                "strategy": {"strategy_id": "plumeusdt_bybit_perp_makerv3", "order_qty": "1000"},
            },
            mode="live",
            force_enable_execution=False,
        )


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


def test_load_runtime_config_merges_shared_telemetry_shipper_table(tmp_path: Path) -> None:
    config_path = tmp_path / "strategy.toml"
    shared_path = tmp_path / "shared.toml"
    config_path.write_text(
        """
[identity]
strategy_id = "plumeusdt_bybit_perp_makerv3"

[strategy]
strategy_id = "plumeusdt_bybit_perp_makerv3"
""".strip()
        + "\n",
        encoding="utf-8",
    )
    shared_path.write_text(
        """
[redis]
host = "redis.internal"
port = 6380

[portfolio]
portfolio_id = "tokenmm"

[telemetry_shipper]
enabled = true
enable_local_persistence = true
fills_db_path = "/var/lib/nautilus/telemetry/tokenmm/fills.sqlite"
orders_db_path = "/var/lib/nautilus/telemetry/tokenmm/orders.sqlite"
quote_cycles_db_path = "/var/lib/nautilus/telemetry/tokenmm/quote_cycles.sqlite"
""".strip()
        + "\n",
        encoding="utf-8",
    )

    merged = run_node._load_runtime_config(
        config_path,
        shared_config_path=shared_path,
    )

    assert merged["redis"]["host"] == "redis.internal"
    assert merged["portfolio"]["portfolio_id"] == "tokenmm"
    assert merged["telemetry_shipper"]["enable_local_persistence"] is True
    assert (
        merged["telemetry_shipper"]["orders_db_path"]
        == "/var/lib/nautilus/telemetry/tokenmm/orders.sqlite"
    )


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
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
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
                "fills_db_path": "/var/lib/nautilus/telemetry/tokenmm/fills.sqlite",
                "orders_db_path": "/var/lib/nautilus/telemetry/tokenmm/orders.sqlite",
                "quote_cycles_db_path": "/var/lib/nautilus/telemetry/tokenmm/quote_cycles.sqlite",
            },
        },
        mode="live",
        force_enable_execution=False,
    )

    config = captured["config"]
    actors = config.actors
    assert len(actors) == 3
    assert {actor.actor_path for actor in actors} == {
        "nautilus_trader.persistence.fills.actor:ExecutionFillPersistenceActor",
        "nautilus_trader.persistence.orders.actor:OrderActionPersistenceActor",
        "nautilus_trader.flux.persistence.quote_cycles.actor:QuoteCyclePersistenceActor",
    }


def test_prepare_telemetry_paths_creates_parent_dirs_when_enabled(tmp_path: Path) -> None:
    fills_path = tmp_path / "telemetry" / "fills.sqlite"
    orders_path = tmp_path / "telemetry" / "orders.sqlite"
    quote_cycles_path = tmp_path / "telemetry" / "quote_cycles.sqlite"
    config = {
        "telemetry_shipper": {
            "enable_local_persistence": True,
            "fills_db_path": str(fills_path),
            "orders_db_path": str(orders_path),
            "quote_cycles_db_path": str(quote_cycles_path),
        },
    }

    run_node._prepare_telemetry_paths(config)

    assert fills_path.parent.is_dir()


def test_build_telemetry_actor_configs_includes_markouts_actor() -> None:
    actors = run_node._build_telemetry_actor_configs(
        {
            "telemetry_shipper": {
                "enable_local_persistence": True,
                "fills_db_path": "/tmp/fills.sqlite",
                "orders_db_path": "/tmp/orders.sqlite",
                "quote_cycles_db_path": "/tmp/quote_cycles.sqlite",
                "markouts_db_path": "/tmp/markouts.sqlite",
                "markout_horizons_s": [30, 60, 120],
            },
        },
    )

    markout_actor = next(
        actor
        for actor in actors
        if actor.actor_path.endswith("markouts.actor:ExecutionMarkoutPersistenceActor")
    )

    assert markout_actor.config["db_path"] == "/tmp/markouts.sqlite"
    assert markout_actor.config["horizons_s"] == [30, 60, 120]


def test_prepare_telemetry_paths_creates_markouts_parent_dir_when_enabled(tmp_path: Path) -> None:
    markouts_path = tmp_path / "telemetry" / "markouts.sqlite"
    config = {
        "telemetry_shipper": {
            "enable_local_persistence": True,
            "markouts_db_path": str(markouts_path),
        },
    }

    run_node._prepare_telemetry_paths(config)

    assert markouts_path.parent.is_dir()


def test_tokenmm_live_config_pins_markout_telemetry_defaults() -> None:
    shared_config = (_repo_root() / "deploy/tokenmm/tokenmm.live.toml").read_text(
        encoding="utf-8",
    )

    assert 'markouts_db_path = "/var/lib/nautilus/telemetry/tokenmm/markouts.sqlite"' in shared_config
    assert "markout_horizons_s = [30, 60, 120]" in shared_config


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


def test_main_exits_with_fatal_code_without_restartable_success(monkeypatch, tmp_path: Path) -> None:
    config_path = tmp_path / "strategy.toml"
    shared_path = tmp_path / "shared.toml"
    config_path.write_text("[identity]\nstrategy_id='strategy_a'\n", encoding="utf-8")
    shared_path.write_text("[redis]\nhost='127.0.0.1'\n", encoding="utf-8")

    class _FatalNode:
        def run(self) -> None:
            raise TradingNodeFatalError("fatal startup failure")

        def dispose(self) -> None:
            disposed.append(True)

    disposed: list[bool] = []
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
    monkeypatch.setattr(run_node, "_load_runtime_config", lambda *args, **kwargs: {})
    monkeypatch.setattr(run_node, "_resolve_mode", lambda *_args, **_kwargs: "live")
    monkeypatch.setattr(run_node, "build_node", lambda *args, **kwargs: _FatalNode())

    with pytest.raises(SystemExit, match="78"):
        run_node.main()

    assert disposed == [True]
