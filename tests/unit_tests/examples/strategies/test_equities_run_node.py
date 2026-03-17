from __future__ import annotations

from decimal import Decimal
from pathlib import Path
from types import SimpleNamespace

import pytest

from flux.runners.equities import run_node
from flux.runners.shared.bootstrap import strategy_startup_lock
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.runners.tokenmm.run_node import _strategy_startup_lock as _tokenmm_strategy_startup_lock
from nautilus_trader.live.node import TradingNodeFatalError
from nautilus_trader.model.identifiers import InstrumentId


class _DummyStrategy:
    def __init__(self) -> None:
        self.params_manager_factory = None
        self.portfolio_inventory_feed: dict[str, object] | None = None

    def set_params_manager_factory(self, factory) -> None:
        self.params_manager_factory = factory

    def configure_portfolio_inventory_feed(self, **kwargs) -> None:
        self.portfolio_inventory_feed = kwargs


def _install_strategy_spec(
    monkeypatch,
    strategy_cls: type[object],
    *,
    config_cls: type[object] | None = None,
    strategy_id: str = "makerv4",
    param_set: str = "makerv4",
    strategy_family: str = "maker_v4",
    strategy_version: str = "v4",
) -> None:
    monkeypatch.setattr(
        run_node,
        "get_strategy_spec",
        lambda name: (
            SimpleNamespace(
                strategy_id=strategy_id,
                strategy_cls=strategy_cls,
                config_cls=config_cls or run_node.MakerV4StrategyConfig,
                param_set=param_set,
                strategy_family=strategy_family,
                strategy_version=strategy_version,
                profile_key="maker_v4",
            )
        ),
        raising=False,
    )


def test_equities_startup_lock_uses_descriptor_specific_lock_dir(tmp_path: Path) -> None:
    config = {"identity": {"strategy_id": "aapl_tradexyz_makerv3"}}

    with strategy_startup_lock(
        config,
        descriptor=get_strategy_set_descriptor("equities"),
        repo_root=tmp_path,
    ):
        assert (
            tmp_path / ".run" / "equities-strategy-locks" / "aapl_tradexyz_makerv3.lock"
        ).exists()


def test_attach_portfolio_inventory_feed_wires_equities_portfolio_reader(monkeypatch) -> None:
    strategy = _DummyStrategy()
    redis_call: dict[str, object] = {}
    redis_client = object()

    def _fake_redis(**kwargs):
        redis_call.update(kwargs)
        return redis_client

    monkeypatch.setattr(run_node.redis, "Redis", _fake_redis)

    run_node._attach_portfolio_inventory_feed(
        strategy=strategy,
        config={"portfolio": {"portfolio_id": "equities", "inventory_stale_after_ms": 2500}},
        redis_cfg={"host": "127.0.0.10", "port": 6381, "db": 4},
        namespace="fluxx",
        schema_version="v2",
    )

    assert redis_call["host"] == "127.0.0.10"
    assert strategy.portfolio_inventory_feed == {
        "allow_partial_global_risk": False,
        "redis_client": redis_client,
        "portfolio_id": "equities",
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
    _install_strategy_spec(
        monkeypatch,
        _CapturedStrategy,
        config_cls=run_node.MakerV3StrategyConfig,
        strategy_id="makerv3",
        param_set="makerv3",
        strategy_family="maker_v3",
        strategy_version="v3",
    )
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL-USD-PERP.HYPERLIQUID"),
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
                "strategy_id": "aapl_tradexyz_makerv3",
                "external_strategy_id": "aapl_tradexyz_makerv3",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False},
            "strategy": {"strategy_id": "aapl_tradexyz_makerv3", "order_qty": "1000"},
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
                strategy_id=name,
                strategy_cls=_CapturedStrategy,
                config_cls=_CapturedStrategyConfig,
                param_set="makerv4",
                strategy_family="maker_v4",
                strategy_version="v4",
                profile_key="maker_v4",
            )
        ),
        raising=False,
    )
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL-USD-PERP.HYPERLIQUID"),
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
                "strategy_id": "aapl_tradexyz_makerv3",
                "external_strategy_id": "aapl_tradexyz_makerv3",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False},
            "strategy": {"strategy_id": "aapl_tradexyz_makerv3", "order_qty": "1000"},
        },
        mode="paper",
        force_enable_execution=False,
    )

    assert registry_calls == ["makerv3"]
    assert isinstance(captured["strategy"], _CapturedStrategy)
    assert isinstance(captured["strategy"].config, _CapturedStrategyConfig)


def test_build_node_injects_makerv3_portfolio_asset_id_from_strategy_contracts(monkeypatch) -> None:
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
    _install_strategy_spec(
        monkeypatch,
        _CapturedStrategy,
        config_cls=_CapturedStrategyConfig,
        strategy_id="makerv3",
        param_set="makerv3",
        strategy_family="maker_v3",
        strategy_version="v3",
    )
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
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
                "strategy_id": "aapl_tradexyz_makerv3",
                "external_strategy_id": "aapl_tradexyz_makerv3",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False},
            "strategy": {"strategy_id": "aapl_tradexyz_makerv3", "order_qty": "1000"},
            "strategy_contracts": [
                {
                    "strategy_id": "aapl_tradexyz_makerv3",
                    "portfolio_asset_id": "AAPL",
                    "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "AAPL.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                    "hedge_account_scope_id": "ibkr.hedge.main",
                },
            ],
        },
        mode="paper",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]
    assert strategy.config.portfolio_asset_id == "AAPL"
    assert strategy.config.execution_account_scope_id == "hyperliquid.xyz.main"


def test_build_node_attaches_profile_account_projection_feed_from_execution_scope(
    monkeypatch,
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
            self.projection_feed_kwargs: dict[str, object] | None = None

        def set_params_manager_factory(self, _factory) -> None:
            return None

        def configure_portfolio_inventory_feed(self, **_kwargs) -> None:
            return None

        def configure_profile_account_projection_feed(self, **kwargs) -> None:
            self.projection_feed_kwargs = kwargs

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_strategy_spec(
        monkeypatch,
        _CapturedStrategy,
        config_cls=_CapturedStrategyConfig,
        strategy_id="makerv3",
        param_set="makerv3",
        strategy_family="maker_v3",
        strategy_version="v3",
    )
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
            data_clients={},
            exec_clients={},
            data_factories={},
            exec_factories={},
        ),
    )
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)
    monkeypatch.setattr(
        run_node.redis,
        "Redis",
        lambda **_kwargs: SimpleNamespace(client_name="projection-redis"),
    )

    run_node.build_node(
        {
            "flux": {"namespace": "flux", "schema_version": "v1"},
            "identity": {
                "strategy_id": "aapl_tradexyz_makerv3",
                "external_strategy_id": "aapl_tradexyz_makerv3",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False},
            "strategy": {"strategy_id": "aapl_tradexyz_makerv3", "order_qty": "1000"},
            "strategy_contracts": [
                {
                    "strategy_id": "aapl_tradexyz_makerv3",
                    "portfolio_asset_id": "AAPL",
                    "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "AAPL.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                    "hedge_account_scope_id": "ibkr.hedge.main",
                },
            ],
        },
        mode="paper",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]
    assert strategy.projection_feed_kwargs is not None
    assert strategy.projection_feed_kwargs["profile_id"] == "equities"
    assert strategy.projection_feed_kwargs["account_scope_id"] == "hyperliquid.xyz.main"
    assert strategy.projection_feed_kwargs["namespace"] == "flux"
    assert strategy.projection_feed_kwargs["schema_version"] == "v1"


def test_load_runtime_config_keeps_strategy_contracts_and_account_scopes(
    tmp_path: Path,
) -> None:
    strategy_path = tmp_path / "strategy.toml"
    shared_path = tmp_path / "shared.toml"
    strategy_path.write_text(
        """
[flux]
mode = "paper"

[identity]
strategy_id = "makerv3"
external_strategy_id = "aapl_tradexyz_makerv3"

[node]
enable_execution = false

[strategy]
strategy_id = "aapl_tradexyz_makerv3"
""".strip()
        + "\n",
        encoding="utf-8",
    )
    shared_path.write_text(
        """
[redis]
host = "redis.internal"
port = 6379
db = 0

[portfolio]
portfolio_id = "equities"

[[account_scopes]]
scope_id = "ibkr.reference.main"
provider = "ibkr"
venue = "IBKR"

[[strategy_contracts]]
strategy_id = "aapl_tradexyz_makerv3"
portfolio_asset_id = "AAPL"
maker_instrument_id = "xyz:AAPL-USD-PERP.HYPERLIQUID"
reference_instrument_id = "AAPL.NASDAQ"
execution_account_scope_id = "hyperliquid.xyz.main"
reference_account_scope_id = "ibkr.reference.main"
hedge_account_scope_id = "ibkr.hedge.main"
""".strip()
        + "\n",
        encoding="utf-8",
    )

    merged = run_node._load_runtime_config(strategy_path, shared_config_path=shared_path)

    assert merged["portfolio"]["portfolio_id"] == "equities"
    assert merged["account_scopes"][0]["scope_id"] == "ibkr.reference.main"
    assert merged["strategy_contracts"][0]["portfolio_asset_id"] == "AAPL"


def test_optional_strategy_config_kwargs_injects_shared_contract_identity_fields(
    tmp_path: Path,
) -> None:
    strategy_path = tmp_path / "strategy.toml"
    shared_path = tmp_path / "shared.toml"
    strategy_path.write_text(
        """
[identity]
strategy_id = "makerv3"
external_strategy_id = "aapl_tradexyz_makerv3"

[strategy]
strategy_id = "aapl_tradexyz_makerv3"
""".strip()
        + "\n",
        encoding="utf-8",
    )
    shared_path.write_text(
        """
[[account_scopes]]
scope_id = "ibkr.reference.main"
provider = "ibkr"
venue = "IBKR"

[[strategy_contracts]]
strategy_id = "aapl_tradexyz_makerv3"
portfolio_asset_id = "AAPL"
maker_instrument_id = "xyz:AAPL-USD-PERP.HYPERLIQUID"
reference_instrument_id = "AAPL.NASDAQ"
execution_account_scope_id = "hyperliquid.xyz.main"
reference_account_scope_id = "ibkr.reference.main"
hedge_account_scope_id = "ibkr.hedge.main"
""".strip()
        + "\n",
        encoding="utf-8",
    )

    merged = run_node._load_runtime_config(strategy_path, shared_config_path=shared_path)

    class _CapturedStrategyConfig:
        def __init__(self, **kwargs) -> None:
            self.__dict__.update(kwargs)

    kwargs = run_node._optional_strategy_config_kwargs(
        config=merged,
        external_strategy_id="aapl_tradexyz_makerv3",
        strategy_spec=SimpleNamespace(config_cls=_CapturedStrategyConfig),
        strategy_cfg={},
    )

    assert kwargs["portfolio_asset_id"] == "AAPL"
    assert kwargs["execution_account_scope_id"] == "hyperliquid.xyz.main"


def test_strategy_spec_capabilities_expose_shared_account_projection_support() -> None:
    spec = run_node.get_strategy_spec("makerv4")

    assert spec.capabilities.uses_profile_account_projection is True
    assert spec.capabilities.supports_immediate_hedge is True


def test_resolve_strategy_spec_uses_explicit_strategy_param_set(monkeypatch) -> None:
    registry_calls: list[str] = []

    monkeypatch.setattr(
        run_node,
        "get_strategy_spec",
        lambda name: (
            registry_calls.append(name)
            or SimpleNamespace(
                strategy_id=name,
                strategy_cls=object,
                config_cls=object,
                param_set=name,
                strategy_family="maker_v4",
                strategy_version="v4",
                profile_key="maker_v4",
            )
        ),
        raising=False,
    )

    spec = run_node._resolve_strategy_spec({"strategy": {"param_set": "makerv4"}})

    assert registry_calls == ["makerv4"]
    assert spec.param_set == "makerv4"
    assert spec.profile_key == "maker_v4"


def test_runtime_params_module_follows_strategy_capabilities_not_param_set() -> None:
    strategy_spec = SimpleNamespace(
        param_set="future_equities_arb",
        capabilities=SimpleNamespace(supports_immediate_hedge=True),
    )

    runtime_params_mod = run_node._runtime_params_module(strategy_spec)

    assert runtime_params_mod.PARAM_SET == "makerv4"


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
            strategy_id="makerv3",
            strategy_cls=_CapturedStrategy,
            config_cls=_CapturedStrategyConfig,
            param_set="makerv3",
            strategy_family="maker_v3",
            strategy_version="v3",
            profile_key="maker_v3",
        ),
        raising=False,
    )
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
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
                    "strategy_id": "aapl_tradexyz_makerv3",
                    "external_strategy_id": "aapl_tradexyz_makerv3",
                    "trader_id": "EQUITIES-LIVE-TRADEXYZ",
                },
                "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
                "node": {"enable_execution": False},
                "strategy": {"strategy_id": "aapl_tradexyz_makerv3", "order_qty": "1000"},
            },
            mode="paper",
            force_enable_execution=False,
        )

    assert captured["strategy"].config.qty_unit == "venue"
    assert "qty_unit missing" in caplog.text
    assert "defaulting to 'venue'" in caplog.text


def test_build_node_defaults_missing_qty_and_order_qty_to_one(monkeypatch) -> None:
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
            strategy_id="makerv3",
            strategy_cls=_CapturedStrategy,
            config_cls=_CapturedStrategyConfig,
            param_set="makerv3",
            strategy_family="maker_v3",
            strategy_version="v3",
            profile_key="maker_v3",
        ),
        raising=False,
    )
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
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
                "strategy_id": "aapl_tradexyz_makerv3",
                "external_strategy_id": "aapl_tradexyz_makerv3",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False},
            "strategy": {"strategy_id": "aapl_tradexyz_makerv3"},
        },
        mode="paper",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]

    assert strategy.config.order_qty == Decimal("1")
    assert strategy.config.qty == Decimal("1")


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
            execution_instrument_id=InstrumentId.from_str("AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
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
                    "strategy_id": "aapl_tradexyz_makerv3",
                    "external_strategy_id": "aapl_tradexyz_makerv3",
                    "trader_id": "EQUITIES-LIVE-TRADEXYZ",
                },
                "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
                "node": {"enable_execution": False},
                "strategy": {
                    "strategy_id": "aapl_tradexyz_makerv3",
                    "order_qty": "1000",
                    "qty_unit": "contracts",
                },
            },
            mode="paper",
            force_enable_execution=False,
        )


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

    maker_instrument_id = InstrumentId.from_str("AAPL-USD-PERP.HYPERLIQUID")

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_strategy_spec(
        monkeypatch,
        _CapturedStrategy,
        config_cls=run_node.MakerV3StrategyConfig,
        strategy_id="makerv3",
        param_set="makerv3",
        strategy_family="maker_v3",
        strategy_version="v3",
    )
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=maker_instrument_id,
            reference_instrument_id=InstrumentId.from_str("AAPL-USD-PERP.HYPERLIQUID"),
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
                "strategy_id": "aapl_tradexyz_makerv3",
                "external_strategy_id": "aapl_tradexyz_makerv3",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": True},
            "strategy": {"strategy_id": "aapl_tradexyz_makerv3", "order_qty": "1000"},
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


def test_build_node_forwards_makerv3_reference_quote_tick_flag(monkeypatch) -> None:
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
    _install_strategy_spec(
        monkeypatch,
        _CapturedStrategy,
        config_cls=run_node.MakerV3StrategyConfig,
        strategy_id="makerv3",
        param_set="makerv3",
        strategy_family="maker_v3",
        strategy_version="v3",
    )
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
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
                "strategy_id": "aapl_tradexyz_makerv3",
                "external_strategy_id": "aapl_tradexyz_makerv3",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": True},
            "strategy": {
                "strategy_id": "aapl_tradexyz_makerv3",
                "order_qty": "1",
                "reference_use_quote_ticks": True,
            },
        },
        mode="live",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]
    assert strategy.config.reference_use_quote_ticks is True


def test_build_node_makerv4_authorizes_maker_and_hedge_instruments(monkeypatch) -> None:
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

    maker_instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    hedge_instrument_id = InstrumentId.from_str("AAPL.NASDAQ")

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    monkeypatch.setattr(
        run_node,
        "get_strategy_spec",
        lambda name: SimpleNamespace(
            strategy_id=name,
            strategy_cls=_CapturedStrategy,
            config_cls=_CapturedStrategyConfig,
            param_set="makerv4",
            strategy_family="maker_v4",
            strategy_version="v4",
            profile_key="maker_v4",
        ),
        raising=False,
    )
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=maker_instrument_id,
            reference_instrument_id=hedge_instrument_id,
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
                "strategy_id": "aapl_tradexyz_makerv4",
                "external_strategy_id": "aapl_tradexyz_makerv4",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": True},
            "strategy": {
                "strategy_id": "aapl_tradexyz_makerv4",
                "param_set": "makerv4",
                "order_qty": "1",
            },
        },
        mode="live",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]
    node_config = captured["node_config"]

    assert strategy.config.allowed_submit_instrument_ids == [
        maker_instrument_id,
        hedge_instrument_id,
    ]
    assert strategy.config.external_order_claims == [
        maker_instrument_id,
        hedge_instrument_id,
    ]
    assert node_config.exec_engine.reconciliation_instrument_ids == [
        maker_instrument_id,
        hedge_instrument_id,
    ]


def test_strategy_startup_lock_is_isolated_from_tokenmm(monkeypatch, tmp_path: Path) -> None:
    config = {"identity": {"strategy_id": "aapl_tradexyz_makerv3"}}

    monkeypatch.setattr(run_node, "_repo_root", lambda: tmp_path)
    from flux.runners.tokenmm import run_node as tokenmm_run_node

    monkeypatch.setattr(tokenmm_run_node, "_repo_root", lambda: tmp_path)

    with _tokenmm_strategy_startup_lock(config), run_node._strategy_startup_lock(config):
        assert (tmp_path / ".run" / "tokenmm-strategy-locks" / "aapl_tradexyz_makerv3.lock").exists()
        assert (tmp_path / ".run" / "equities-strategy-locks" / "aapl_tradexyz_makerv3.lock").exists()


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
    _install_strategy_spec(
        monkeypatch,
        _CapturedStrategy,
        config_cls=run_node.MakerV3StrategyConfig,
        strategy_id="makerv3",
        param_set="makerv3",
        strategy_family="maker_v3",
        strategy_version="v3",
    )
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL-USD-PERP.HYPERLIQUID"),
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
                "strategy_id": "aapl_tradexyz_makerv3",
                "external_strategy_id": "aapl_tradexyz_makerv3",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": True},
            "strategy": {
                "strategy_id": "aapl_tradexyz_makerv3",
                "order_qty": "1000",
                "cancel_all_instrument_orders": True,
            },
        },
        mode="live",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]

    assert strategy.config.cancel_all_instrument_orders is True


def test_build_node_honors_reference_use_quote_ticks_override(monkeypatch) -> None:
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
    _install_strategy_spec(
        monkeypatch,
        _CapturedStrategy,
        config_cls=run_node.MakerV3StrategyConfig,
        strategy_id="makerv3",
        param_set="makerv3",
        strategy_family="maker_v3",
        strategy_version="v3",
    )
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
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
                "strategy_id": "aapl_tradexyz_makerv3",
                "external_strategy_id": "aapl_tradexyz_makerv3",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": True},
            "strategy": {
                "strategy_id": "aapl_tradexyz_makerv3",
                "order_qty": "1",
                "reference_use_quote_ticks": True,
            },
        },
        mode="live",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]

    assert strategy.config.reference_use_quote_ticks is True


def test_build_node_passes_makerv4_hedge_config_fields(monkeypatch) -> None:
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
        lambda name: SimpleNamespace(
            strategy_id=name,
            strategy_cls=_CapturedStrategy,
            config_cls=_CapturedStrategyConfig,
            param_set="makerv4",
            strategy_family="maker_v4",
            strategy_version="v4",
            profile_key="maker_v4",
        ),
        raising=False,
    )
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
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
                "strategy_id": "aapl_tradexyz_makerv4",
                "external_strategy_id": "aapl_tradexyz_makerv4",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": True},
            "strategy": {
                "strategy_id": "aapl_tradexyz_makerv4",
                "param_set": "makerv4",
                "order_qty": "1",
                "outside_rth_hedge_enabled": True,
                "hedge_price_tick_size": "0.05",
                "hedge_min_share_increment": "1",
                "max_ibkr_quote_age_ms": 2500,
                "max_ibkr_spread_bps": "45",
                "ibkr_primary_exchange": "NASDAQ",
            },
        },
        mode="live",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]
    assert strategy.config.outside_rth_hedge_enabled is True
    assert strategy.config.hedge_price_tick_size == Decimal("0.05")
    assert strategy.config.max_ibkr_quote_age_ms == 2500
    assert strategy.config.max_ibkr_spread_bps == Decimal(45)
    assert strategy.config.ibkr_primary_exchange == "NASDAQ"


def test_build_node_derives_makerv4_ibkr_reference_instrument_from_primary_exchange(
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

    maker_instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    expected_reference_instrument_id = InstrumentId.from_str("AAPL.NASDAQ")

    def _resolve_strategy_venues(**kwargs):
        effective_config = kwargs["config"]
        captured["resolved_ibkr_instrument_id"] = effective_config["node"]["venues"]["IBKR"][
            "instrument_id"
        ]
        return SimpleNamespace(
            execution_instrument_id=maker_instrument_id,
            reference_instrument_id=InstrumentId.from_str(
                str(captured["resolved_ibkr_instrument_id"]),
            ),
            data_clients={},
            exec_clients={},
            data_factories={},
            exec_factories={},
        )

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    monkeypatch.setattr(
        run_node,
        "get_strategy_spec",
        lambda name: SimpleNamespace(
            strategy_id=name,
            strategy_cls=_CapturedStrategy,
            config_cls=_CapturedStrategyConfig,
            param_set="makerv4",
            strategy_family="maker_v4",
            strategy_version="v4",
            profile_key="maker_v4",
        ),
        raising=False,
    )
    monkeypatch.setattr(run_node, "resolve_strategy_venues", _resolve_strategy_venues)
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)

    run_node.build_node(
        {
            "flux": {"namespace": "flux", "schema_version": "v1"},
            "identity": {
                "strategy_id": "aapl_tradexyz_makerv4",
                "external_strategy_id": "aapl_tradexyz_makerv4",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "venues": {
                "execution_venue": "HYPERLIQUID",
                "reference_venue": "IBKR",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {
                "enable_execution": False,
                "venues": {
                    "HYPERLIQUID": {
                        "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    },
                    "IBKR": {
                        "instrument_id": "AAPL.NASDAQ",
                    },
                },
            },
            "strategy": {
                "strategy_id": "aapl_tradexyz_makerv4",
                "param_set": "makerv4",
                "order_qty": "1",
                "ibkr_primary_exchange": "NASDAQ",
            },
        },
        mode="paper",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]

    assert captured["resolved_ibkr_instrument_id"] == "AAPL.NASDAQ"
    assert strategy.config.reference_instrument_id == expected_reference_instrument_id


def test_build_node_uses_ibkr_reference_instrument_id(monkeypatch) -> None:
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

    maker_instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    ref_instrument_id = InstrumentId.from_str("AAPL.NASDAQ")

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=maker_instrument_id,
            reference_instrument_id=ref_instrument_id,
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
                "strategy_id": "aapl_tradexyz_makerv3",
                "external_strategy_id": "aapl_tradexyz_makerv3",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False},
            "strategy": {"strategy_id": "aapl_tradexyz_makerv3", "order_qty": "1000"},
        },
        mode="paper",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]

    assert strategy.config.maker_instrument_id == maker_instrument_id
    assert strategy.config.reference_instrument_id == ref_instrument_id


def test_build_node_force_enable_execution_overrides_explicit_false_for_makerv4(monkeypatch) -> None:
    captured: dict[str, object] = {}

    class _CapturedNode:
        def __init__(self, config) -> None:
            captured["node_config"] = config
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
            execution_instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
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
                "strategy_id": "aapl_tradexyz_makerv4",
                "external_strategy_id": "aapl_tradexyz_makerv4",
                "trader_id": "EQUITIES-LIVE-AAPL-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False},
            "strategy": {
                "strategy_id": "aapl_tradexyz_makerv4",
                "param_set": "makerv4",
                "order_qty": "1",
            },
        },
        mode="live",
        force_enable_execution=True,
    )

    assert captured["enable_execution"] is True


def test_effective_venue_resolution_config_promotes_ibkr_execution_for_immediate_hedge_spec() -> None:
    strategy_spec = SimpleNamespace(
        param_set="future_equities_arb",
        capabilities=SimpleNamespace(supports_immediate_hedge=True),
    )
    config = {
        "node": {
            "venues": {
                "HYPERLIQUID": {
                    "instrument_id": "xyz:NVDA-USD-PERP.HYPERLIQUID",
                    "execution": True,
                },
                "IBKR": {
                    "adapter": "interactive_brokers",
                    "instrument_id": "NVDA.NASDAQ",
                    "execution": False,
                    "ibg_client_id": 23,
                },
            },
        },
        "strategy": {
            "ibkr_primary_exchange": "NASDAQ",
        },
    }

    effective = run_node._effective_venue_resolution_config(
        config=config,
        strategy_spec=strategy_spec,
    )

    assert effective["node"]["venues"]["IBKR"]["instrument_id"] == "NVDA.NASDAQ"
    assert effective["node"]["venues"]["IBKR"]["execution"] is True
    assert config["node"]["venues"]["IBKR"]["execution"] is False


def test_effective_venue_resolution_config_preserves_strategy_ibkr_client_id_for_immediate_hedge_spec() -> None:
    strategy_spec = SimpleNamespace(
        param_set="future_equities_arb",
        capabilities=SimpleNamespace(supports_immediate_hedge=True),
    )
    config = {
        "identity": {
            "strategy_id": "nvda_tradexyz_makerv4",
            "external_strategy_id": "nvda_tradexyz_makerv4",
        },
        "node": {
            "venues": {
                "HYPERLIQUID": {
                    "instrument_id": "xyz:NVDA-USD-PERP.HYPERLIQUID",
                    "execution": True,
                },
                "IBKR": {
                    "adapter": "interactive_brokers",
                    "instrument_id": "NVDA.NASDAQ",
                    "execution": False,
                    "ibg_client_id": 23,
                },
            },
        },
        "strategy": {
            "ibkr_primary_exchange": "NASDAQ",
        },
        "account_scopes": [
            {
                "scope_id": "ibkr.reference.main",
                "provider": "ibkr",
                "venue": "IBKR",
                "ibg_host": "127.0.0.1",
                "ibg_port": 4002,
                "ibg_client_id": 107,
                "account_id": "U10015777",
                "dockerized_gateway": {
                    "trading_mode": "live",
                    "manage_container": False,
                },
            },
            {
                "scope_id": "ibkr.hedge.main",
                "provider": "ibkr",
                "venue": "IBKR",
                "ibg_host": "127.0.0.1",
                "ibg_port": 4002,
                "ibg_client_id": 108,
                "account_id": "U10015777",
                "dockerized_gateway": {
                    "trading_mode": "live",
                    "manage_container": False,
                },
            },
        ],
        "strategy_contracts": [
            {
                "strategy_id": "nvda_tradexyz_makerv4",
                "portfolio_asset_id": "NVDA",
                "maker_instrument_id": "xyz:NVDA-USD-PERP.HYPERLIQUID",
                "reference_instrument_id": "NVDA.NASDAQ",
                "execution_account_scope_id": "hyperliquid.xyz.main",
                "reference_account_scope_id": "ibkr.reference.main",
                "hedge_account_scope_id": "ibkr.hedge.main",
            },
        ],
    }

    effective = run_node._effective_venue_resolution_config(
        config=config,
        strategy_spec=strategy_spec,
    )

    ibkr_cfg = effective["node"]["venues"]["IBKR"]
    assert ibkr_cfg["ibg_host"] == "127.0.0.1"
    assert ibkr_cfg["ibg_client_id"] == 23
    assert ibkr_cfg["account_id"] == "U10015777"
    assert ibkr_cfg["dockerized_gateway"] == {
        "trading_mode": "live",
        "manage_container": False,
    }
    assert "ibg_port" not in ibkr_cfg


def test_build_node_keeps_ibkr_reference_balance_snapshot_provider_profile_owned_for_makerv4(
    monkeypatch,
) -> None:
    captured: dict[str, object] = {}

    class _CapturedNode:
        def __init__(self, config) -> None:
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
            self.reference_balance_snapshot_provider = None

        def set_params_manager_factory(self, _factory) -> None:
            return None

        def configure_portfolio_inventory_feed(self, **_kwargs) -> None:
            return None

        def configure_reference_balance_snapshot_provider(self, provider) -> None:
            self.reference_balance_snapshot_provider = provider

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_strategy_spec(monkeypatch, _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
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
                "strategy_id": "aapl_tradexyz_makerv4",
                "external_strategy_id": "aapl_tradexyz_makerv4",
                "trader_id": "EQUITIES-LIVE-AAPL-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "venues": {
                "execution_venue": "HYPERLIQUID",
                "reference_venue": "IBKR",
            },
            "node": {
                "enable_execution": False,
                "venues": {
                    "HYPERLIQUID": {
                        "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                        "execution": True,
                    },
                    "IBKR": {
                        "adapter": "interactive_brokers",
                        "instrument_id": "AAPL.NASDAQ",
                        "ibg_client_id": 7,
                        "execution": False,
                        "dockerized_gateway": {
                            "trading_mode": "live",
                            "read_only_api": True,
                        },
                    },
                },
            },
            "strategy": {
                "strategy_id": "aapl_tradexyz_makerv4",
                "param_set": "makerv4",
                "order_qty": "1",
            },
        },
        mode="paper",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]
    assert strategy.reference_balance_snapshot_provider is None


def test_attach_reference_balance_snapshot_provider_uses_none_port_with_dockerized_gateway(
    monkeypatch,
) -> None:
    captured: dict[str, object] = {}

    class _CapturedStrategy:
        def configure_reference_balance_snapshot_provider(self, provider) -> None:
            captured["provider"] = provider

    monkeypatch.setattr(
        run_node,
        "get_cached_ibkr_reference_balance_provider",
        lambda config: captured.setdefault("provider_config", config) or object(),
    )

    run_node._attach_reference_balance_snapshot_provider(
        strategy=_CapturedStrategy(),
        config={
            "venues": {
                "reference_venue": "IBKR",
            },
            "node": {
                "venues": {
                    "IBKR": {
                        "adapter": "interactive_brokers",
                        "ibg_host": "127.0.0.1",
                        "ibg_port": 4002,
                        "ibg_client_id": 7,
                        "dockerized_gateway": {
                            "trading_mode": "live",
                            "read_only_api": True,
                        },
                    },
                },
            },
        },
        strategy_spec=SimpleNamespace(
            capabilities=SimpleNamespace(uses_profile_account_projection=False),
        ),
    )

    provider_config = captured["provider_config"]
    assert provider_config.dockerized_gateway is not None
    assert provider_config.ibg_port is None
    assert provider_config.ibg_client_id == 7
    assert captured["provider"] is not None


def test_build_node_real_makerv4_strategy_satisfies_trader_registration_contract(
    monkeypatch,
) -> None:
    registered: dict[str, object] = {}

    class _CapturedNode:
        def __init__(self, config) -> None:
            self.trader = SimpleNamespace(add_strategy=self._add_strategy)

        def _add_strategy(self, strategy) -> None:
            registered["is_running"] = strategy.is_running
            registered["is_disposed"] = strategy.is_disposed
            registered["id"] = strategy.id
            registered["order_id_tag"] = strategy.order_id_tag
            registered["register"] = strategy.register

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
            execution_instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
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
                "strategy_id": "aapl_tradexyz_makerv4",
                "external_strategy_id": "aapl_tradexyz_makerv4",
                "trader_id": "EQUITIES-LIVE-AAPL-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False},
            "strategy": {
                "strategy_id": "aapl_tradexyz_makerv4",
                "param_set": "makerv4",
                "order_qty": "1",
            },
        },
        mode="paper",
        force_enable_execution=False,
    )

    assert registered["is_running"] is False
    assert registered["is_disposed"] is False
    assert str(registered["id"]).startswith("aapl_tradexyz_makerv4-")
    assert callable(registered["register"])


def test_build_node_real_makerv4_strategy_receives_profile_account_projection_feed(
    monkeypatch,
) -> None:
    captured: dict[str, object] = {}
    projection_redis = SimpleNamespace(client_name="projection-redis")

    class _CapturedNode:
        def __init__(self, config) -> None:
            self.trader = SimpleNamespace(
                add_strategy=lambda strategy: captured.setdefault("strategy", strategy),
            )

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
            execution_instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
            reference_instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
            data_clients={},
            exec_clients={},
            data_factories={},
            exec_factories={},
        ),
    )
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)
    monkeypatch.setattr(run_node.redis, "Redis", lambda **_kwargs: projection_redis)

    run_node.build_node(
        {
            "flux": {"namespace": "flux", "schema_version": "v1"},
            "identity": {
                "strategy_id": "aapl_tradexyz_makerv4",
                "external_strategy_id": "aapl_tradexyz_makerv4",
                "trader_id": "EQUITIES-LIVE-AAPL-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False},
            "strategy": {
                "strategy_id": "aapl_tradexyz_makerv4",
                "param_set": "makerv4",
                "order_qty": "1",
            },
            "strategy_contracts": [
                {
                    "strategy_id": "aapl_tradexyz_makerv4",
                    "portfolio_asset_id": "AAPL",
                    "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "AAPL.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                    "hedge_account_scope_id": "ibkr.hedge.main",
                },
            ],
        },
        mode="paper",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]
    assert strategy._profile_account_projection_client is projection_redis
    assert strategy._profile_account_projection_profile_id == "equities"
    assert strategy._profile_account_projection_account_scope_id == "hyperliquid.xyz.main"


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
