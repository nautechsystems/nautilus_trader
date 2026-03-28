from __future__ import annotations

import asyncio
from contextlib import contextmanager
from decimal import Decimal
from pathlib import Path
from types import SimpleNamespace

import pytest

from flux.runners.equities import run_node
from flux.runners.shared.bootstrap import strategy_startup_lock
from flux.runners.shared.quote_feed_supervisor import QuoteFeedClaimSpec
from flux.runners.shared.quote_feed_supervisor import QuoteFeedIdentity
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.strategies.equities_maker import EquitiesMakerStrategyConfig
from flux.strategies.equities_taker import EquitiesTakerStrategyConfig
from flux.runners.tokenmm.run_node import _strategy_startup_lock as _tokenmm_strategy_startup_lock
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
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
    profile_key: str = "maker_v4",
) -> None:
    spec = SimpleNamespace(
        strategy_id=strategy_id,
        strategy_cls=strategy_cls,
        config_cls=config_cls or run_node.MakerV4StrategyConfig,
        param_set=param_set,
        strategy_family=strategy_family,
        strategy_version=strategy_version,
        profile_key=profile_key,
        capabilities=SimpleNamespace(
            publishes_local_inventory=param_set not in {"equities_maker", "equities_taker"},
            uses_profile_account_projection=True,
            supports_immediate_hedge=param_set in {"makerv4", "equities_maker", "equities_taker"},
        ),
    )
    monkeypatch.setattr(
        run_node,
        "get_strategy_spec",
        lambda _name: spec,
        raising=False,
    )
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_spec_for_strategy_id",
        lambda _strategy_id, default=None: spec,
        raising=False,
    )


def _install_grouped_strategy_specs(
    monkeypatch,
    strategy_cls: type[object],
) -> None:
    specs = {
        "equities_maker": SimpleNamespace(
            strategy_id="equities_maker",
            strategy_cls=strategy_cls,
            config_cls=EquitiesMakerStrategyConfig,
            param_set="equities_maker",
            strategy_family="equities_maker",
            strategy_version="v1",
            profile_key="equities_maker",
            capabilities=SimpleNamespace(
                publishes_local_inventory=False,
                uses_profile_account_projection=True,
                supports_immediate_hedge=True,
            ),
        ),
        "equities_taker": SimpleNamespace(
            strategy_id="equities_taker",
            strategy_cls=strategy_cls,
            config_cls=EquitiesTakerStrategyConfig,
            param_set="equities_taker",
            strategy_family="equities_taker",
            strategy_version="v1",
            profile_key="equities_taker",
            capabilities=SimpleNamespace(
                publishes_local_inventory=False,
                uses_profile_account_projection=True,
                supports_immediate_hedge=True,
            ),
        ),
    }

    monkeypatch.setattr(
        run_node,
        "get_strategy_spec",
        lambda name: specs[name],
        raising=False,
    )
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_spec_for_strategy_id",
        lambda strategy_id, default=None: specs[
            "equities_taker" if strategy_id.endswith("_taker") else "equities_maker"
        ],
        raising=False,
    )


def _split_equities_config(
    *,
    strategy_id: str,
    param_set: str,
    trader_id: str,
    portfolio_asset_id: str,
    maker_instrument_id: str,
    reference_instrument_id: str,
    execution_account_scope_id: str,
) -> dict[str, object]:
    base_group_id = strategy_id.removesuffix("_maker").removesuffix("_taker")
    maker_strategy_id = f"{base_group_id}_maker"
    taker_strategy_id = f"{base_group_id}_taker"
    return {
        "flux": {"namespace": "flux", "schema_version": "v1"},
        "identity": {
            "strategy_id": strategy_id,
            "external_strategy_id": strategy_id,
            "trader_id": trader_id,
        },
        "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
        "node": {"enable_execution": True},
        "portfolio": {"profile_owned_account_projections": True},
        "strategy": {
            "strategy_id": strategy_id,
            "param_set": param_set,
            "order_qty": "1",
            "qty": "1",
        },
        "strategy_contracts": [
            {
                "strategy_id": maker_strategy_id,
                "portfolio_asset_id": portfolio_asset_id,
                "maker_instrument_id": maker_instrument_id,
                "reference_instrument_id": reference_instrument_id,
                "execution_account_scope_id": execution_account_scope_id,
                "reference_account_scope_id": "ibkr.reference.main",
                "hedge_account_scope_id": "ibkr.hedge.main",
            },
            {
                "strategy_id": taker_strategy_id,
                "portfolio_asset_id": portfolio_asset_id,
                "maker_instrument_id": maker_instrument_id,
                "reference_instrument_id": reference_instrument_id,
                "execution_account_scope_id": execution_account_scope_id,
                "reference_account_scope_id": "ibkr.reference.main",
                "hedge_account_scope_id": "ibkr.hedge.main",
            },
        ],
    }


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
    resolve_calls: list[str] = []

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
        "resolve_strategy_spec_for_strategy_id",
        lambda strategy_id, default=None: (
            resolve_calls.append(strategy_id)
            or SimpleNamespace(
                strategy_id="makerv3",
                strategy_cls=_CapturedStrategy,
                config_cls=_CapturedStrategyConfig,
                param_set="makerv3",
                strategy_family="maker_v3",
                strategy_version="v3",
                profile_key="maker_v3",
                capabilities=SimpleNamespace(
                    publishes_local_inventory=True,
                    uses_profile_account_projection=True,
                    supports_immediate_hedge=False,
                ),
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

    assert resolve_calls == ["aapl_tradexyz_makerv3"]
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
                    "maker_venue": "HYPERLIQUID",
                    "maker_symbol": "AAPL",
                    "market_type": "perp",
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
                    "maker_venue": "HYPERLIQUID",
                    "maker_symbol": "AAPL",
                    "market_type": "perp",
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


def test_build_node_omits_local_inventory_fields_for_equities_maker(monkeypatch) -> None:
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
        config_cls=EquitiesMakerStrategyConfig,
        strategy_id="equities_maker",
        param_set="equities_maker",
        strategy_family="equities_maker",
        strategy_version="v1",
        profile_key="equities_maker",
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
                "strategy_id": "aapl_tradexyz_maker",
                "external_strategy_id": "aapl_tradexyz_maker",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False},
            "strategy": {
                "strategy_id": "aapl_tradexyz_maker",
                "order_qty": "1000",
                "des_qty_local": "5",
                "max_qty_local": "10",
                "max_skew_bps_local": "7",
            },
        },
        mode="paper",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]
    assert isinstance(strategy, _CapturedStrategy)
    assert isinstance(strategy.config, EquitiesMakerStrategyConfig)
    assert not hasattr(strategy.config, "des_qty_local")
    assert not hasattr(strategy.config, "max_qty_local")
    assert not hasattr(strategy.config, "max_skew_bps_local")


def test_build_node_omits_local_inventory_and_maker_quote_fields_for_equities_taker(
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
        config_cls=EquitiesTakerStrategyConfig,
        strategy_id="equities_taker",
        param_set="equities_taker",
        strategy_family="equities_taker",
        strategy_version="v1",
        profile_key="equities_taker",
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
                "strategy_id": "aapl_tradexyz_taker",
                "external_strategy_id": "aapl_tradexyz_taker",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False},
            "strategy": {
                "strategy_id": "aapl_tradexyz_taker",
                "order_qty": "1000",
                "des_qty_local": "5",
                "max_qty_local": "10",
                "max_skew_bps_local": "7",
                "bid_edge1": "5",
                "ask_edge1": "5",
                "n_orders1": "1",
            },
        },
        mode="paper",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]
    assert isinstance(strategy, _CapturedStrategy)
    assert isinstance(strategy.config, EquitiesTakerStrategyConfig)
    assert not hasattr(strategy.config, "des_qty_local")
    assert not hasattr(strategy.config, "max_qty_local")
    assert not hasattr(strategy.config, "max_skew_bps_local")
    assert not hasattr(strategy.config, "bid_edge1")
    assert not hasattr(strategy.config, "ask_edge1")
    assert not hasattr(strategy.config, "n_orders1")


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

[[account_scopes]]
scope_id = "binance.futures.main"
provider = "binance"
venue = "BINANCE_PERP"
api_key_env = "EQUITIES_BINANCE_API_KEY"
api_secret_env = "EQUITIES_BINANCE_API_SECRET"
account_type = "USDT_FUTURES"
private_api_family = "PORTFOLIO_MARGIN"
base_url_http = "https://papi.binance.com"
recv_window_ms = 5000

[[strategy_contracts]]
strategy_id = "aapl_tradexyz_makerv3"
portfolio_asset_id = "AAPL"
maker_venue = "HYPERLIQUID"
maker_symbol = "AAPL"
market_type = "perp"
maker_instrument_id = "xyz:AAPL-USD-PERP.HYPERLIQUID"
reference_instrument_id = "AAPL.NASDAQ"
execution_account_scope_id = "hyperliquid.xyz.main"
reference_account_scope_id = "ibkr.reference.main"
hedge_account_scope_id = "ibkr.hedge.main"

[[strategy_contracts]]
strategy_id = "amzn_binance_perp_maker"
portfolio_asset_id = "AMZN"
maker_venue = "BINANCE_PERP"
maker_symbol = "AMZNUSDT"
market_type = "perp"
maker_instrument_id = "AMZNUSDT-PERP.BINANCE_PERP"
reference_instrument_id = "AMZN.NASDAQ"
execution_account_scope_id = "binance.futures.main"
reference_account_scope_id = "ibkr.reference.main"
hedge_account_scope_id = "ibkr.hedge.main"
""".strip()
        + "\n",
        encoding="utf-8",
    )

    merged = run_node._load_runtime_config(strategy_path, shared_config_path=shared_path)

    assert merged["portfolio"]["portfolio_id"] == "equities"
    assert merged["account_scopes"][0]["scope_id"] == "ibkr.reference.main"
    assert merged["account_scopes"][1]["base_url_http"] == "https://papi.binance.com"
    assert merged["strategy_contracts"][0]["portfolio_asset_id"] == "AAPL"
    assert merged["strategy_contracts"][0]["maker_venue"] == "HYPERLIQUID"
    assert merged["strategy_contracts"][1]["strategy_id"] == "amzn_binance_perp_maker"
    assert merged["strategy_contracts"][1]["maker_symbol"] == "AMZNUSDT"


def test_load_runtime_config_merges_missing_portfolio_keys_from_shared_config(
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
external_strategy_id = "aapl_tradexyz_maker"

[portfolio]
portfolio_id = "equities"

[strategy]
strategy_id = "aapl_tradexyz_maker"
""".strip()
        + "\n",
        encoding="utf-8",
    )
    shared_path.write_text(
        """
[portfolio]
inventory_stale_after_ms = 30000
allow_partial_global_risk = true
""".strip()
        + "\n",
        encoding="utf-8",
    )

    merged = run_node._load_runtime_config(strategy_path, shared_config_path=shared_path)

    assert merged["portfolio"]["portfolio_id"] == "equities"
    assert merged["portfolio"]["inventory_stale_after_ms"] == 30000
    assert merged["portfolio"]["allow_partial_global_risk"] is True


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
maker_venue = "HYPERLIQUID"
maker_symbol = "AAPL"
market_type = "perp"
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


def test_resolve_strategy_spec_uses_strategy_id_suffix_for_equities_maker() -> None:
    spec = run_node._resolve_strategy_spec(
        {
            "identity": {
                "strategy_id": "aapl_tradexyz_maker",
                "external_strategy_id": "aapl_tradexyz_maker",
            },
            "strategy": {
                "strategy_id": "aapl_tradexyz_maker",
            },
        },
    )

    assert spec.strategy_id == "equities_maker"
    assert spec.param_set == "equities_maker"


def test_resolve_strategy_spec_uses_strategy_id_suffix_for_equities_taker() -> None:
    spec = run_node._resolve_strategy_spec(
        {
            "identity": {
                "strategy_id": "aapl_tradexyz_taker",
                "external_strategy_id": "aapl_tradexyz_taker",
            },
            "strategy": {
                "strategy_id": "aapl_tradexyz_taker",
            },
        },
    )

    assert spec.strategy_id == "equities_taker"
    assert spec.param_set == "equities_taker"


def test_runtime_params_module_follows_strategy_capabilities_not_param_set() -> None:
    strategy_spec = SimpleNamespace(
        param_set="future_equities_arb",
        capabilities=SimpleNamespace(supports_immediate_hedge=True),
    )

    runtime_params_mod = run_node._runtime_params_module(strategy_spec)

    assert runtime_params_mod.PARAM_SET == "makerv4"


def test_runtime_params_module_prefers_equities_maker_family_module_when_available() -> None:
    strategy_spec = SimpleNamespace(
        param_set="equities_maker",
        capabilities=SimpleNamespace(supports_immediate_hedge=True),
    )

    runtime_params_mod = run_node._runtime_params_module(strategy_spec)

    assert runtime_params_mod.PARAM_SET == "equities_maker"
    assert "execution_mode" not in runtime_params_mod.RUNTIME_PARAM_DEFAULTS


def test_runtime_params_module_prefers_equities_taker_family_module_when_available() -> None:
    strategy_spec = SimpleNamespace(
        param_set="equities_taker",
        capabilities=SimpleNamespace(supports_immediate_hedge=True),
    )

    runtime_params_mod = run_node._runtime_params_module(strategy_spec)

    assert runtime_params_mod.PARAM_SET == "equities_taker"
    assert "execution_mode" not in runtime_params_mod.RUNTIME_PARAM_DEFAULTS
    assert "bid_edge1" not in runtime_params_mod.RUNTIME_PARAM_DEFAULTS


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


def test_build_node_passes_equities_maker_hedge_config_fields(monkeypatch) -> None:
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
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)

    run_node.build_node(
        {
            "flux": {"namespace": "flux", "schema_version": "v1"},
            "identity": {
                "strategy_id": "aapl_tradexyz_maker",
                "external_strategy_id": "aapl_tradexyz_maker",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": True},
            "strategy": {
                "strategy_id": "aapl_tradexyz_maker",
                "param_set": "equities_maker",
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


def test_build_node_passes_equities_taker_family_config_fields(monkeypatch) -> None:
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
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)

    run_node.build_node(
        {
            "flux": {"namespace": "flux", "schema_version": "v1"},
            "identity": {
                "strategy_id": "aapl_tradexyz_taker",
                "external_strategy_id": "aapl_tradexyz_taker",
                "trader_id": "EQUITIES-LIVE-TRADEXYZ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": True},
            "strategy": {
                "strategy_id": "aapl_tradexyz_taker",
                "param_set": "equities_taker",
                "order_qty": "1",
                "bid_edge_take_bps": "6",
                "ask_edge_take_bps": "7",
                "take_cooldown_ms": 2500,
                "outside_rth_hedge_enabled": True,
                "hedge_price_tick_size": "0.05",
                "hedge_min_share_increment": "1",
                "max_ibkr_quote_age_ms": 2500,
                "max_ibkr_spread_bps": "45",
                "ibkr_hedge_route": "BLUEOCEAN",
                "ibkr_primary_exchange": "NASDAQ",
            },
        },
        mode="live",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]
    assert strategy.config.bid_edge_take_bps == 6.0
    assert strategy.config.ask_edge_take_bps == 7.0
    assert strategy.config.take_cooldown_ms == 2500
    assert strategy.config.outside_rth_hedge_enabled is True
    assert strategy.config.hedge_price_tick_size == Decimal("0.05")
    assert strategy.config.max_ibkr_quote_age_ms == 2500
    assert strategy.config.max_ibkr_spread_bps == Decimal(45)
    assert strategy.config.ibkr_hedge_route == "BLUEOCEAN"
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


def test_build_node_uses_explicit_contract_reference_for_binance_perp_route(monkeypatch) -> None:
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

    expected_maker_instrument_id = InstrumentId.from_str("PLTRUSDT-PERP.BINANCE_PERP")
    expected_reference_instrument_id = InstrumentId.from_str("PLTR.NASDAQ")

    def _resolve_strategy_venues(**kwargs):
        effective_config = kwargs["config"]
        captured["resolved_execution_instrument_id"] = effective_config["node"]["venues"][
            "BINANCE_PERP"
        ]["instrument_id"]
        captured["resolved_ibkr_instrument_id"] = effective_config["node"]["venues"]["IBKR"][
            "instrument_id"
        ]
        return SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str(
                str(captured["resolved_execution_instrument_id"]),
            ),
            reference_instrument_id=InstrumentId.from_str(
                str(captured["resolved_ibkr_instrument_id"]),
            ),
            data_clients={},
            exec_clients={},
            data_factories={},
            exec_factories={},
        )

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_strategy_spec(
        monkeypatch,
        _CapturedStrategy,
        config_cls=_CapturedStrategyConfig,
    )
    monkeypatch.setattr(run_node, "resolve_strategy_venues", _resolve_strategy_venues)
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)

    run_node.build_node(
        {
            "flux": {"namespace": "flux", "schema_version": "v1"},
            "identity": {
                "strategy_id": "pltr_binance_perp_makerv4",
                "external_strategy_id": "pltr_binance_perp_makerv4",
                "trader_id": "EQUITIES-LIVE-PLTR-BINANCE",
            },
            "venues": {
                "execution_venue": "BINANCE_PERP",
                "reference_venue": "IBKR",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {
                "enable_execution": False,
                "venues": {
                    "BINANCE_PERP": {
                        "adapter": "binance",
                        "instrument_id": "AMZNUSDT-PERP.BINANCE_PERP",
                        "account_type": "USDT_FUTURES",
                        "execution": True,
                    },
                    "IBKR": {
                        "adapter": "interactive_brokers",
                        "instrument_id": "PLTR.SMART",
                        "execution": False,
                        "ibg_client_id": 23,
                    },
                },
            },
            "strategy": {
                "strategy_id": "pltr_binance_perp_makerv4",
                "param_set": "makerv4",
                "order_qty": "1",
                "ibkr_primary_exchange": "NASDAQ",
            },
            "account_scopes": [
                {
                    "scope_id": "ibkr.reference.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4001,
                    "ibg_client_id": 107,
                    "account_id": "U10015777",
                },
                {
                    "scope_id": "ibkr.hedge.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4001,
                    "ibg_client_id": 108,
                    "account_id": "U10015777",
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
                    "hedge_account_scope_id": "ibkr.hedge.main",
                },
            ],
        },
        mode="paper",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]

    assert captured["resolved_execution_instrument_id"] == "PLTRUSDT-PERP.BINANCE_PERP"
    assert captured["resolved_ibkr_instrument_id"] == "PLTR.NASDAQ"
    assert strategy.config.maker_instrument_id == expected_maker_instrument_id
    assert strategy.config.reference_instrument_id == expected_reference_instrument_id


def test_build_node_promotes_contract_maker_venue_over_stale_top_level_execution_venue(
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

    def _resolve_strategy_venues(**kwargs):
        effective_config = kwargs["config"]
        captured["execution_venue"] = effective_config["venues"]["execution_venue"]
        captured["execution_instrument_id"] = effective_config["node"]["venues"]["BINANCE_PERP"][
            "instrument_id"
        ]
        return SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str(
                str(captured["execution_instrument_id"]),
            ),
            reference_instrument_id=InstrumentId.from_str("PLTR.NASDAQ"),
            data_clients={},
            exec_clients={},
            data_factories={},
            exec_factories={},
        )

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_strategy_spec(
        monkeypatch,
        _CapturedStrategy,
        config_cls=_CapturedStrategyConfig,
    )
    monkeypatch.setattr(run_node, "resolve_strategy_venues", _resolve_strategy_venues)
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)

    run_node.build_node(
        {
            "flux": {"namespace": "flux", "schema_version": "v1"},
            "identity": {
                "strategy_id": "pltr_binance_perp_makerv4",
                "external_strategy_id": "pltr_binance_perp_makerv4",
                "trader_id": "EQUITIES-LIVE-PLTR-BINANCE",
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
                        "instrument_id": "xyz:PLTR-USD-PERP.HYPERLIQUID",
                    },
                    "BINANCE_PERP": {
                        "adapter": "binance",
                        "instrument_id": "AMZNUSDT-PERP.BINANCE_PERP",
                        "account_type": "USDT_FUTURES",
                        "execution": True,
                    },
                    "IBKR": {
                        "adapter": "interactive_brokers",
                        "instrument_id": "PLTR.SMART",
                        "execution": False,
                        "ibg_client_id": 23,
                    },
                },
            },
            "strategy": {
                "strategy_id": "pltr_binance_perp_makerv4",
                "param_set": "makerv4",
                "order_qty": "1",
                "ibkr_primary_exchange": "NASDAQ",
            },
            "account_scopes": [
                {
                    "scope_id": "ibkr.reference.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4001,
                    "ibg_client_id": 107,
                    "account_id": "U10015777",
                },
                {
                    "scope_id": "ibkr.hedge.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4001,
                    "ibg_client_id": 108,
                    "account_id": "U10015777",
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
                    "hedge_account_scope_id": "ibkr.hedge.main",
                },
            ],
        },
        mode="paper",
        force_enable_execution=False,
    )

    assert captured["execution_venue"] == "BINANCE_PERP"
    assert captured["execution_instrument_id"] == "PLTRUSDT-PERP.BINANCE_PERP"


def test_build_node_clears_stale_execution_flags_when_contract_promotes_new_maker_venue(
    monkeypatch,
) -> None:
    captured: dict[str, object] = {}

    class _CapturedNode:
        def __init__(self, *args, **kwargs) -> None:
            _ = args, kwargs
            self.trader = SimpleNamespace(add_strategy=lambda _strategy: None)

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

    def _resolve_strategy_venues(**kwargs):
        effective_config = kwargs["config"]
        captured["execution_venue"] = effective_config["venues"]["execution_venue"]
        captured["hyperliquid_execution"] = effective_config["node"]["venues"]["HYPERLIQUID"][
            "execution"
        ]
        captured["binance_execution"] = effective_config["node"]["venues"]["BINANCE_PERP"][
            "execution"
        ]
        return SimpleNamespace(
            execution_instrument_id=InstrumentId.from_str("PLTRUSDT-PERP.BINANCE_PERP"),
            reference_instrument_id=InstrumentId.from_str("PLTR.NASDAQ"),
            data_clients={},
            exec_clients={},
            data_factories={},
            exec_factories={},
        )

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_strategy_spec(
        monkeypatch,
        _CapturedStrategy,
        config_cls=_CapturedStrategyConfig,
    )
    monkeypatch.setattr(run_node, "resolve_strategy_venues", _resolve_strategy_venues)
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)

    run_node.build_node(
        {
            "flux": {"namespace": "flux", "schema_version": "v1"},
            "identity": {
                "strategy_id": "pltr_binance_perp_makerv4",
                "external_strategy_id": "pltr_binance_perp_makerv4",
                "trader_id": "EQUITIES-LIVE-PLTR-BINANCE",
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
                        "instrument_id": "xyz:PLTR-USD-PERP.HYPERLIQUID",
                        "execution": True,
                    },
                    "BINANCE_PERP": {
                        "adapter": "binance",
                        "instrument_id": "AMZNUSDT-PERP.BINANCE_PERP",
                        "account_type": "USDT_FUTURES",
                        "execution": False,
                    },
                    "IBKR": {
                        "adapter": "interactive_brokers",
                        "instrument_id": "PLTR.SMART",
                        "execution": False,
                        "ibg_client_id": 23,
                    },
                },
            },
            "strategy": {
                "strategy_id": "pltr_binance_perp_makerv4",
                "param_set": "makerv4",
                "order_qty": "1",
                "ibkr_primary_exchange": "NASDAQ",
            },
            "account_scopes": [
                {
                    "scope_id": "ibkr.reference.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4001,
                    "ibg_client_id": 107,
                    "account_id": "U10015777",
                },
                {
                    "scope_id": "binance.futures.main",
                    "provider": "binance",
                    "venue": "BINANCE_PERP",
                    "account_type": "USDT_FUTURES",
                    "api_key_env": "EQUITIES_BINANCE_API_KEY",
                    "api_secret_env": "EQUITIES_BINANCE_API_SECRET",
                },
            ],
            "strategy_contracts": [
                {
                    "strategy_id": "pltr_binance_perp_makerv4",
                    "portfolio_asset_id": "PLTR",
                    "maker_venue": "BINANCE_PERP",
                    "maker_symbol": "PLTRUSDT",
                    "maker_instrument_id": "PLTRUSDT-PERP.BINANCE_PERP",
                    "reference_instrument_id": "PLTR.NASDAQ",
                    "execution_account_scope_id": "binance.futures.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                },
            ],
        },
        mode="paper",
        force_enable_execution=False,
    )

    assert captured["execution_venue"] == "BINANCE_PERP"
    assert captured["hyperliquid_execution"] is False
    assert captured["binance_execution"] is True


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
                "ibg_port": 4001,
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
                "ibg_port": 4001,
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
                "maker_venue": "HYPERLIQUID",
                "maker_symbol": "NVDA",
                "market_type": "perp",
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
    assert ibkr_cfg["ibg_port"] == 4001


def test_effective_venue_resolution_config_falls_back_to_identity_strategy_id_when_external_id_missing() -> None:
    strategy_spec = SimpleNamespace(
        param_set="future_equities_arb",
        capabilities=SimpleNamespace(supports_immediate_hedge=True),
    )
    config = {
        "identity": {
            "strategy_id": "nvda_tradexyz_makerv4",
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
                    "ibg_client_id": "",
                },
            },
        },
        "strategy": {
            "ibkr_primary_exchange": "NASDAQ",
        },
        "account_scopes": [
            {
                "scope_id": "ibkr.hedge.main",
                "provider": "ibkr",
                "venue": "IBKR",
                "ibg_host": "127.0.0.1",
                "ibg_port": 4001,
                "ibg_client_id": 108,
                "account_id": "U10015777",
                "dockerized_gateway": {
                    "trading_mode": "live",
                    "container_name": "ibg-main",
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
                "reference_account_scope_id": "ibkr.hedge.main",
                "hedge_account_scope_id": "ibkr.hedge.main",
            },
        ],
    }

    effective = run_node._effective_venue_resolution_config(
        config=config,
        strategy_spec=strategy_spec,
    )

    ibkr_cfg = effective["node"]["venues"]["IBKR"]
    assert ibkr_cfg["execution"] is True
    assert ibkr_cfg["ibg_host"] == "127.0.0.1"
    assert ibkr_cfg["ibg_port"] == 4001
    assert ibkr_cfg["ibg_client_id"] == 108
    assert ibkr_cfg["account_id"] == "U10015777"
    assert ibkr_cfg["dockerized_gateway"] == {
        "trading_mode": "live",
        "container_name": "ibg-main",
    }


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


def test_attach_reference_balance_snapshot_provider_preserves_explicit_port_with_dockerized_gateway(
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
                        "ibg_port": 4001,
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
    assert provider_config.ibg_port == 4001
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
                    "maker_venue": "HYPERLIQUID",
                    "maker_symbol": "AAPL",
                    "market_type": "perp",
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


def test_build_node_filters_shared_external_orders_for_primary_same_asset_variant(
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

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_strategy_spec(
        monkeypatch,
        _CapturedStrategy,
        config_cls=EquitiesMakerStrategyConfig,
        strategy_id="equities_maker",
        param_set="equities_maker",
        strategy_family="equities_maker",
        strategy_version="v1",
        profile_key="equities_maker",
    )
    maker_instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    reference_instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=maker_instrument_id,
            reference_instrument_id=reference_instrument_id,
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
                "strategy_id": "aapl_tradexyz_maker",
                "external_strategy_id": "aapl_tradexyz_maker",
                "trader_id": "EQUITIES-LIVE-AAPL-TRADEXYZ-MAKER",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": True},
            "strategy": {
                "strategy_id": "aapl_tradexyz_maker",
                "param_set": "equities_maker",
                "order_qty": "1",
            },
            "strategy_contracts": [
                {
                    "strategy_id": "aapl_tradexyz_maker",
                    "portfolio_asset_id": "AAPL",
                    "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "AAPL.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                    "hedge_account_scope_id": "ibkr.hedge.main",
                },
                {
                    "strategy_id": "aapl_tradexyz_taker",
                    "portfolio_asset_id": "AAPL",
                    "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "AAPL.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                    "hedge_account_scope_id": "ibkr.hedge.main",
                },
            ],
        },
        mode="live",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]
    node_config = captured["node_config"]

    assert strategy.config.external_order_claims == []
    assert node_config.exec_engine.reconciliation is True
    assert node_config.exec_engine.filter_unclaimed_external_orders is True
    assert node_config.exec_engine.reconciliation_instrument_ids == [
        maker_instrument_id,
        reference_instrument_id,
    ]


def test_build_node_filters_shared_external_orders_for_secondary_same_asset_variant(
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

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_strategy_spec(
        monkeypatch,
        _CapturedStrategy,
        config_cls=EquitiesTakerStrategyConfig,
        strategy_id="equities_taker",
        param_set="equities_taker",
        strategy_family="equities_taker",
        strategy_version="v1",
        profile_key="equities_taker",
    )
    maker_instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    reference_instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=maker_instrument_id,
            reference_instrument_id=reference_instrument_id,
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
                "strategy_id": "aapl_tradexyz_taker",
                "external_strategy_id": "aapl_tradexyz_taker",
                "trader_id": "EQUITIES-LIVE-AAPL-TRADEXYZ-TAKER",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": True},
            "strategy": {
                "strategy_id": "aapl_tradexyz_taker",
                "param_set": "equities_taker",
                "order_qty": "1",
            },
            "strategy_contracts": [
                {
                    "strategy_id": "aapl_tradexyz_maker",
                    "portfolio_asset_id": "AAPL",
                    "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "AAPL.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                    "hedge_account_scope_id": "ibkr.hedge.main",
                },
                {
                    "strategy_id": "aapl_tradexyz_taker",
                    "portfolio_asset_id": "AAPL",
                    "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                    "reference_instrument_id": "AAPL.NASDAQ",
                    "execution_account_scope_id": "hyperliquid.xyz.main",
                    "reference_account_scope_id": "ibkr.reference.main",
                    "hedge_account_scope_id": "ibkr.hedge.main",
                },
            ],
        },
        mode="live",
        force_enable_execution=False,
    )

    strategy = captured["strategy"]
    node_config = captured["node_config"]

    assert strategy.config.external_order_claims == []
    assert node_config.exec_engine.reconciliation is True
    assert node_config.exec_engine.filter_unclaimed_external_orders is True
    assert node_config.exec_engine.reconciliation_instrument_ids == [
        maker_instrument_id,
        reference_instrument_id,
    ]


def test_build_grouped_node_builds_one_shared_node_for_tradexyz_node_group(
    monkeypatch,
) -> None:
    captured: dict[str, object] = {
        "strategies": [],
        "data_factories": [],
        "exec_factories": [],
    }
    attach_calls = {
        "runtime": [],
        "inventory": [],
        "projection": [],
        "reference": [],
    }

    class _CapturedStrategy:
        def __init__(self, *, config) -> None:
            self.config = config

    class _CapturedNode:
        def __init__(self, config) -> None:
            captured["node_config"] = config
            self.trader = SimpleNamespace(add_strategy=self._add_strategy)

        def _add_strategy(self, strategy) -> None:
            captured["strategies"].append(strategy)

        def add_data_client_factory(self, venue, factory) -> None:
            captured["data_factories"].append((venue, factory))

        def add_exec_client_factory(self, venue, factory) -> None:
            captured["exec_factories"].append((venue, factory))

        def build(self) -> None:
            captured["build_called"] = True

    resolve_calls: list[dict[str, object]] = []
    maker_instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    reference_instrument_id = InstrumentId.from_str("AAPL.NASDAQ")

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_grouped_strategy_specs(monkeypatch, _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **kwargs: resolve_calls.append(kwargs)
        or SimpleNamespace(
            execution_instrument_id=maker_instrument_id,
            reference_instrument_id=reference_instrument_id,
            data_clients={"HYPERLIQUID": object()},
            exec_clients={"HYPERLIQUID": object()},
            data_factories={"HYPERLIQUID": "data-factory"},
            exec_factories={"HYPERLIQUID": "exec-factory"},
        ),
    )
    monkeypatch.setattr(run_node, "_register_cash_borrowing_venues", lambda **_kwargs: None)
    monkeypatch.setattr(
        run_node,
        "_attach_runtime_params_manager",
        lambda **kwargs: attach_calls["runtime"].append(
            (
                kwargs["strategy"].config.external_strategy_id,
                kwargs["strategy_spec"].strategy_id,
            ),
        ),
    )
    monkeypatch.setattr(
        run_node,
        "_attach_portfolio_inventory_feed",
        lambda **kwargs: attach_calls["inventory"].append(
            kwargs["strategy"].config.external_strategy_id,
        ),
    )
    monkeypatch.setattr(
        run_node,
        "_attach_profile_account_projection_feed",
        lambda **kwargs: attach_calls["projection"].append(
            kwargs["strategy"].config.external_strategy_id,
        ),
    )
    monkeypatch.setattr(
        run_node,
        "_attach_reference_balance_snapshot_provider",
        lambda **kwargs: attach_calls["reference"].append(
            kwargs["strategy"].config.external_strategy_id,
        ),
    )

    node = run_node.build_grouped_node(
        (
            _split_equities_config(
                strategy_id="aapl_tradexyz_maker",
                param_set="equities_maker",
                trader_id="EQUITIES-LIVE-AAPL-TRADEXYZ-MAKER",
                portfolio_asset_id="AAPL",
                maker_instrument_id=str(maker_instrument_id),
                reference_instrument_id=str(reference_instrument_id),
                execution_account_scope_id="hyperliquid.xyz.main",
            ),
            _split_equities_config(
                strategy_id="aapl_tradexyz_taker",
                param_set="equities_taker",
                trader_id="EQUITIES-LIVE-AAPL-TRADEXYZ-TAKER",
                portfolio_asset_id="AAPL",
                maker_instrument_id=str(maker_instrument_id),
                reference_instrument_id=str(reference_instrument_id),
                execution_account_scope_id="hyperliquid.xyz.main",
            ),
        ),
        mode="live",
        force_enable_execution=False,
    )

    assert node is not None
    assert captured["build_called"] is True
    assert len(resolve_calls) == 1
    assert len(captured["strategies"]) == 2
    assert {
        strategy.config.external_strategy_id
        for strategy in captured["strategies"]
    } == {"aapl_tradexyz_maker", "aapl_tradexyz_taker"}
    assert {
        strategy.config.external_strategy_id: strategy.config.external_order_claims
        for strategy in captured["strategies"]
    } == {
        "aapl_tradexyz_maker": [],
        "aapl_tradexyz_taker": [],
    }

    node_config = captured["node_config"]
    assert str(node_config.trader_id) == "EQUITIES-LIVE-AAPL-TRADEXYZ"
    assert node_config.message_bus.streams_prefix == "flux:v1:in:stream:live:aapl_tradexyz"
    assert node_config.exec_engine.reconciliation_instrument_ids == [
        maker_instrument_id,
        reference_instrument_id,
    ]
    assert captured["data_factories"] == [("HYPERLIQUID", "data-factory")]
    assert captured["exec_factories"] == [("HYPERLIQUID", "exec-factory")]
    assert attach_calls["runtime"] == [
        ("aapl_tradexyz_maker", "equities_maker"),
        ("aapl_tradexyz_taker", "equities_taker"),
    ]
    assert attach_calls["inventory"] == ["aapl_tradexyz_maker", "aapl_tradexyz_taker"]
    assert attach_calls["projection"] == ["aapl_tradexyz_maker", "aapl_tradexyz_taker"]
    assert attach_calls["reference"] == ["aapl_tradexyz_maker", "aapl_tradexyz_taker"]


def test_build_grouped_node_shared_recovery_attaches_one_quote_feed_supervisor_to_siblings(
    monkeypatch,
) -> None:
    class _LoopProbe:
        def __init__(self) -> None:
            self.calls: list[object] = []

        def call_soon_threadsafe(self, callback) -> None:
            self.calls.append(callback)

    loop_probe = _LoopProbe()
    captured: dict[str, object] = {"strategies": []}
    maker_instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    reference_instrument_id = InstrumentId.from_str("AAPL.NASDAQ")

    class _CapturedStrategy:
        def __init__(self, *, config) -> None:
            self.config = config
            self.quote_feed_supervisor = None
            self.quote_feed_control_emitter = None

        def set_params_manager_factory(self, _factory) -> None:
            return None

        def configure_portfolio_inventory_feed(self, **_kwargs) -> None:
            return None

        def configure_quote_feed_runtime(self, *, supervisor, control_emitter) -> None:
            self.quote_feed_supervisor = supervisor
            self.quote_feed_control_emitter = control_emitter

        def quote_feed_claim_specs(self) -> tuple[QuoteFeedClaimSpec, ...]:
            return (
                QuoteFeedClaimSpec(
                    feed_identity=QuoteFeedIdentity(
                        scope="hyperliquid.xyz.main",
                        instrument_id=self.config.maker_instrument_id,
                        topic="maker_quote_ticks",
                    ),
                    claimant_id=self.config.external_strategy_id,
                    unusable_after_ms=3_000,
                    blocker_key="hyperliquid.xyz.main",
                ),
                QuoteFeedClaimSpec(
                    feed_identity=QuoteFeedIdentity(
                        scope="ibkr.shared_publisher",
                        instrument_id=self.config.reference_instrument_id,
                        topic="reference_quote_ticks",
                    ),
                    claimant_id=self.config.external_strategy_id,
                    unusable_after_ms=1_000,
                    blocker_key="ibkr.shared_publisher",
                    node_scoped_lifecycle=False,
                ),
            )

    class _CapturedNode:
        def __init__(self, config) -> None:
            self.trader = SimpleNamespace(
                add_strategy=lambda strategy: captured["strategies"].append(strategy),
            )

        def add_data_client_factory(self, _venue, _factory) -> None:
            return None

        def add_exec_client_factory(self, _venue, _factory) -> None:
            return None

        def build(self) -> None:
            captured["build_called"] = True

        def get_event_loop(self):
            return loop_probe

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_grouped_strategy_specs(monkeypatch, _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=maker_instrument_id,
            reference_instrument_id=reference_instrument_id,
            data_clients={},
            exec_clients={},
            data_factories={},
            exec_factories={},
        ),
    )
    monkeypatch.setattr(run_node, "_register_cash_borrowing_venues", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_profile_account_projection_feed", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_reference_balance_snapshot_provider", lambda **_kwargs: None)

    run_node.build_grouped_node(
        (
            _split_equities_config(
                strategy_id="aapl_tradexyz_maker",
                param_set="equities_maker",
                trader_id="EQUITIES-LIVE-AAPL-TRADEXYZ-MAKER",
                portfolio_asset_id="AAPL",
                maker_instrument_id=str(maker_instrument_id),
                reference_instrument_id=str(reference_instrument_id),
                execution_account_scope_id="hyperliquid.xyz.main",
            ),
            _split_equities_config(
                strategy_id="aapl_tradexyz_taker",
                param_set="equities_taker",
                trader_id="EQUITIES-LIVE-AAPL-TRADEXYZ-TAKER",
                portfolio_asset_id="AAPL",
                maker_instrument_id=str(maker_instrument_id),
                reference_instrument_id=str(reference_instrument_id),
                execution_account_scope_id="hyperliquid.xyz.main",
            ),
        ),
        mode="live",
        force_enable_execution=False,
    )

    strategies = captured["strategies"]
    assert captured["build_called"] is True
    assert len(strategies) == 2
    assert strategies[0].quote_feed_supervisor is strategies[1].quote_feed_supervisor
    assert strategies[0].quote_feed_control_emitter is strategies[1].quote_feed_control_emitter

    supervisor = strategies[0].quote_feed_supervisor
    control_emitter = strategies[0].quote_feed_control_emitter
    maker_feed = QuoteFeedIdentity(
        scope="hyperliquid.xyz.main",
        instrument_id=maker_instrument_id,
        topic="maker_quote_ticks",
    )
    maker_snapshot = supervisor.snapshot(
        maker_feed
    )
    reference_snapshot = supervisor.snapshot(
        QuoteFeedIdentity(
            scope="ibkr.shared_publisher",
            instrument_id=reference_instrument_id,
            topic="reference_quote_ticks",
        )
    )

    assert maker_snapshot.desired is False
    assert reference_snapshot.desired is False
    assert maker_snapshot.claimant_ids == ()
    assert reference_snapshot.claimant_ids == ()

    control_emitter.ingest_result(
        maker_feed,
        now_ns=10_000,
        ok=False,
        error_summary="transport_failed",
    )
    assert len(loop_probe.calls) == 1
    assert supervisor.snapshot(maker_feed).attempt_count == 0
    loop_probe.calls.pop()()
    assert supervisor.snapshot(maker_feed).attempt_count == 0
    assert supervisor.snapshot(maker_feed).last_error_summary is None
    assert (
        control_emitter.ingest_result(
            QuoteFeedIdentity(
                scope="ibkr.shared_publisher",
                instrument_id=reference_instrument_id,
                topic="reference_quote_ticks",
            ),
            now_ns=20_000,
            ok=False,
            error_summary="publisher_down",
    )
        is None
    )
    assert loop_probe.calls == []


def test_build_grouped_node_wraps_hyperliquid_data_factory_for_quote_feed_result_ingress(
    monkeypatch,
) -> None:
    captured: dict[str, object] = {
        "data_factories": [],
        "strategies": [],
    }
    maker_instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    reference_instrument_id = InstrumentId.from_str("AAPL.NASDAQ")

    class _CapturedStrategy:
        def __init__(self, *, config) -> None:
            self.config = config

        def set_params_manager_factory(self, _factory) -> None:
            return None

        def configure_portfolio_inventory_feed(self, **_kwargs) -> None:
            return None

        def configure_quote_feed_runtime(self, *, supervisor, control_emitter) -> None:
            self.quote_feed_supervisor = supervisor
            self.quote_feed_control_emitter = control_emitter

        def quote_feed_claim_specs(self) -> tuple[QuoteFeedClaimSpec, ...]:
            return (
                QuoteFeedClaimSpec(
                    feed_identity=QuoteFeedIdentity(
                        scope="hyperliquid.xyz.main",
                        instrument_id=self.config.maker_instrument_id,
                        topic="maker_quote_ticks",
                    ),
                    claimant_id=self.config.external_strategy_id,
                    unusable_after_ms=3_000,
                    blocker_key="hyperliquid.xyz.main",
                ),
            )

    class _CapturedNode:
        def __init__(self, config) -> None:
            self.trader = SimpleNamespace(
                add_strategy=lambda strategy: captured["strategies"].append(strategy),
            )

        def add_data_client_factory(self, venue, factory) -> None:
            captured["data_factories"].append((venue, factory))

        def add_exec_client_factory(self, _venue, _factory) -> None:
            return None

        def build(self) -> None:
            captured["build_called"] = True

    attached: dict[str, object] = {}

    class _HyperliquidFactory:
        @staticmethod
        def create(**_kwargs):
            class _Client:
                def set_quote_feed_result_ingress(self, ingress) -> None:
                    attached["ingress"] = ingress

            return _Client()

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    monkeypatch.setattr(run_node, "HyperliquidLiveDataClientFactory", _HyperliquidFactory)
    _install_grouped_strategy_specs(monkeypatch, _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=maker_instrument_id,
            reference_instrument_id=reference_instrument_id,
            data_clients={"HYPERLIQUID": object()},
            exec_clients={},
            data_factories={"HYPERLIQUID": _HyperliquidFactory},
            exec_factories={},
        ),
    )
    monkeypatch.setattr(run_node, "_register_cash_borrowing_venues", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_profile_account_projection_feed", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_reference_balance_snapshot_provider", lambda **_kwargs: None)

    run_node.build_grouped_node(
        (
            _split_equities_config(
                strategy_id="aapl_tradexyz_maker",
                param_set="equities_maker",
                trader_id="EQUITIES-LIVE-AAPL-TRADEXYZ-MAKER",
                portfolio_asset_id="AAPL",
                maker_instrument_id=str(maker_instrument_id),
                reference_instrument_id=str(reference_instrument_id),
                execution_account_scope_id="hyperliquid.xyz.main",
            ),
            _split_equities_config(
                strategy_id="aapl_tradexyz_taker",
                param_set="equities_taker",
                trader_id="EQUITIES-LIVE-AAPL-TRADEXYZ-TAKER",
                portfolio_asset_id="AAPL",
                maker_instrument_id=str(maker_instrument_id),
                reference_instrument_id=str(reference_instrument_id),
                execution_account_scope_id="hyperliquid.xyz.main",
            ),
        ),
        mode="live",
        force_enable_execution=False,
    )

    assert captured["build_called"] is True
    assert len(captured["data_factories"]) == 1
    venue, factory = captured["data_factories"][0]
    assert venue == "HYPERLIQUID"
    assert factory is not _HyperliquidFactory

    factory.create(
        loop=None,
        name="hyperliquid",
        config=SimpleNamespace(),
        msgbus=None,
        cache=None,
        clock=None,
    )

    assert callable(attached["ingress"])


def test_attach_quote_feed_runtime_keeps_non_node_scoped_reference_feed_live_on_start_and_stop() -> None:
    executed_commands: list[object] = []
    attached_topics: list[InstrumentId] = []
    detached_topics: list[InstrumentId] = []
    reference_instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    reference_feed = QuoteFeedIdentity(
        scope="ibkr.shared_publisher",
        instrument_id=reference_instrument_id,
        topic="reference_quote_ticks",
    )

    class _CapturedStrategy:
        def __init__(self) -> None:
            self.quote_feed_supervisor = None
            self.quote_feed_control_emitter = None

        def configure_quote_feed_runtime(self, *, supervisor, control_emitter) -> None:
            self.quote_feed_supervisor = supervisor
            self.quote_feed_control_emitter = control_emitter

        def quote_feed_claim_specs(self) -> tuple[QuoteFeedClaimSpec, ...]:
            return (
                QuoteFeedClaimSpec(
                    feed_identity=reference_feed,
                    claimant_id="aapl_tradexyz_maker",
                    unusable_after_ms=1_000,
                    blocker_key="ibkr.shared_publisher",
                    node_scoped_lifecycle=False,
                ),
            )

        def on_start(self) -> None:
            attached_topics.append(reference_instrument_id)
            self.quote_feed_supervisor.register_claimant(
                reference_feed,
                claimant_id="aapl_tradexyz_maker",
                unusable_after_ms=1_000,
                blocker_key="ibkr.shared_publisher",
            )

        def on_stop(self) -> None:
            detached_topics.append(reference_instrument_id)
            self.quote_feed_supervisor.unregister_claimant(
                reference_feed,
                claimant_id="aapl_tradexyz_maker",
            )

    node = SimpleNamespace(
        kernel=SimpleNamespace(
            data_engine=SimpleNamespace(execute=lambda command: executed_commands.append(command)),
            clock=SimpleNamespace(timestamp_ns=lambda: 123_456_789),
        ),
    )
    supervisor = run_node.NodeQuoteFeedSupervisor()
    control_emitter = run_node.QuoteFeedControlEmitter(
        node_scoped_id="aapl_tradexyz",
        sink=lambda command: run_node._emit_quote_feed_command(node=node, command=command),
    )
    strategy = _CapturedStrategy()

    run_node._attach_quote_feed_runtime(
        strategy=strategy,
        supervisor=supervisor,
        control_emitter=control_emitter,
    )

    strategy.on_start()
    strategy.on_stop()

    assert attached_topics == [reference_instrument_id]
    assert detached_topics == [reference_instrument_id]
    assert [type(command) for command in executed_commands] == [
        SubscribeQuoteTicks,
        UnsubscribeQuoteTicks,
    ]
    assert supervisor.snapshot(reference_feed).desired is False


def test_schedule_quote_feed_result_on_node_loop_skips_closed_loop() -> None:
    ingress_calls: list[dict[str, object]] = []

    class _ClosedLoop:
        def is_closed(self) -> bool:
            return True

        def call_soon_threadsafe(self, callback) -> None:
            raise AssertionError(f"should not schedule callback={callback!r}")

    node = SimpleNamespace(get_event_loop=lambda: _ClosedLoop())

    result = run_node._schedule_quote_feed_result_on_node_loop(
        node=node,
        ingress=lambda **kwargs: ingress_calls.append(kwargs),
        now_ns=10_000,
        ok=False,
        error_summary="loop_closed",
    )

    assert result is None
    assert ingress_calls == []


def test_schedule_quote_feed_result_on_node_loop_ignores_runtime_error() -> None:
    ingress_calls: list[dict[str, object]] = []

    class _RaisingLoop:
        def is_closed(self) -> bool:
            return False

        def call_soon_threadsafe(self, callback) -> None:
            raise RuntimeError(f"loop is closing callback={callback!r}")

    node = SimpleNamespace(get_event_loop=lambda: _RaisingLoop())

    result = run_node._schedule_quote_feed_result_on_node_loop(
        node=node,
        ingress=lambda **kwargs: ingress_calls.append(kwargs),
        now_ns=10_000,
        ok=False,
        error_summary="runtime_error",
    )

    assert result is None
    assert ingress_calls == []


def test_schedule_quote_feed_result_on_node_loop_preserves_extra_result_payload() -> None:
    ingress_calls: list[dict[str, object]] = []

    class _LoopProbe:
        def is_closed(self) -> bool:
            return False

        def call_soon_threadsafe(self, callback) -> None:
            callback()

    node = SimpleNamespace(get_event_loop=lambda: _LoopProbe())

    result = run_node._schedule_quote_feed_result_on_node_loop(
        node=node,
        ingress=lambda **kwargs: ingress_calls.append(kwargs),
        now_ns=10_000,
        ok=True,
        error_summary=None,
        instrument_id="BTC-USD-PERP.HYPERLIQUID",
        status="replayed",
        cache_refreshed=True,
        result={"status": "replayed"},
    )

    assert result is None
    assert ingress_calls == [
        {
            "now_ns": 10_000,
            "ok": True,
            "error_summary": None,
            "instrument_id": "BTC-USD-PERP.HYPERLIQUID",
            "status": "replayed",
            "cache_refreshed": True,
            "result": {"status": "replayed"},
        },
    ]


def test_node_scoped_quote_feed_result_routes_drop_ambiguous_same_instrument_feeds() -> None:
    instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    first_feed = QuoteFeedIdentity(
        scope="hyperliquid.xyz.main",
        instrument_id=instrument_id,
        topic="maker_quote_ticks",
    )
    second_feed = QuoteFeedIdentity(
        scope="hyperliquid.xyz.backup",
        instrument_id=instrument_id,
        topic="maker_quote_ticks",
    )

    class _Strategy:
        def quote_feed_claim_specs(self) -> tuple[QuoteFeedClaimSpec, ...]:
            return (
                QuoteFeedClaimSpec(
                    feed_identity=first_feed,
                    claimant_id="first",
                    unusable_after_ms=3_000,
                ),
                QuoteFeedClaimSpec(
                    feed_identity=second_feed,
                    claimant_id="second",
                    unusable_after_ms=3_000,
                ),
            )

    result = run_node._node_scoped_quote_feed_result_routes((_Strategy(),))

    assert result == {}


def test_build_quote_feed_result_ingress_routes_unique_feed_identity() -> None:
    instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    feed_identity = QuoteFeedIdentity(
        scope="hyperliquid.xyz.main",
        instrument_id=instrument_id,
        topic="maker_quote_ticks",
    )
    ingress_calls: list[dict[str, object]] = []

    control_emitter = run_node.QuoteFeedControlEmitter(node_scoped_id="aapl_tradexyz")
    control_emitter.register_result_ingress(
        feed_identity,
        lambda **kwargs: ingress_calls.append(kwargs),
    )

    ingress = run_node._build_quote_feed_result_ingress(
        control_emitter=control_emitter,
        claimed_feed_identities_by_instrument={
            instrument_id.value: feed_identity,
        },
    )

    result = ingress(
        now_ns=10_000,
        ok=False,
        error_summary="transport_unhealthy",
        instrument_id=instrument_id.value,
        status="transport_unhealthy",
        cache_refreshed=False,
        result={"status": "transport_unhealthy"},
    )

    assert result is None
    assert ingress_calls == [
        {
            "now_ns": 10_000,
            "ok": False,
            "error_summary": "transport_unhealthy",
            "instrument_id": instrument_id.value,
            "status": "transport_unhealthy",
            "cache_refreshed": False,
            "result": {"status": "transport_unhealthy"},
        },
    ]


@pytest.mark.asyncio
async def test_emit_quote_feed_command_reset_prefers_live_client_recovery_and_preserves_result_payload() -> None:
    feed_identity = QuoteFeedIdentity(
        scope="hyperliquid.xyz.main",
        instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
        topic="maker_quote_ticks",
    )
    recovered: list[InstrumentId] = []
    ingress_calls: list[dict[str, object]] = []

    async def recover_quote_ticks(instrument_id: InstrumentId) -> dict[str, object]:
        recovered.append(instrument_id)
        return {
            "instrument_id": instrument_id.value,
            "ok": True,
            "status": "replayed",
            "error_summary": None,
            "cache_refreshed": True,
        }

    node = SimpleNamespace(
        get_event_loop=lambda: asyncio.get_running_loop(),
        kernel=SimpleNamespace(
            data_engine=SimpleNamespace(
                execute=lambda command: (_ for _ in ()).throw(
                    AssertionError(f"unexpected command={command!r}"),
                ),
                routing_map={
                    feed_identity.instrument_id.venue: SimpleNamespace(
                        recover_quote_ticks=recover_quote_ticks,
                    ),
                },
            ),
            clock=SimpleNamespace(timestamp_ns=lambda: 123_456_789),
        ),
    )

    control_emitter: run_node.QuoteFeedControlEmitter | None = None

    def sink(command) -> None:
        run_node._emit_quote_feed_command(
            node=node,
            command=command,
            control_emitter=control_emitter,
        )

    control_emitter = run_node.QuoteFeedControlEmitter(
        node_scoped_id="aapl_tradexyz",
        sink=sink,
    )
    control_emitter.register_result_ingress(
        feed_identity,
        lambda **kwargs: ingress_calls.append(kwargs),
    )

    control_emitter.reset(feed_identity)
    await asyncio.sleep(0)

    assert recovered == [feed_identity.instrument_id]
    assert ingress_calls == [
        {
            "now_ns": 123_456_789,
            "ok": True,
            "error_summary": None,
            "instrument_id": feed_identity.instrument_id.value,
            "status": "replayed",
            "cache_refreshed": True,
            "result": {
                "instrument_id": feed_identity.instrument_id.value,
                "ok": True,
                "status": "replayed",
                "error_summary": None,
                "cache_refreshed": True,
            },
        },
    ]


@pytest.mark.asyncio
async def test_emit_quote_feed_command_reset_does_not_duplicate_result_when_live_client_emits_ingress() -> None:
    feed_identity = QuoteFeedIdentity(
        scope="hyperliquid.xyz.main",
        instrument_id=InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID"),
        topic="maker_quote_ticks",
    )
    ingress_calls: list[dict[str, object]] = []
    result_payload = {
        "instrument_id": feed_identity.instrument_id.value,
        "ok": True,
        "status": "replayed",
        "error_summary": None,
        "cache_refreshed": True,
    }

    live_client = SimpleNamespace()

    async def recover_quote_ticks(instrument_id: InstrumentId) -> dict[str, object]:
        live_client._quote_feed_result_ingress(
            now_ns=123_456_789,
            ok=True,
            error_summary=None,
            instrument_id=instrument_id.value,
            status="replayed",
            cache_refreshed=True,
            result=dict(result_payload),
        )
        return dict(result_payload)

    live_client.recover_quote_ticks = recover_quote_ticks
    live_client.has_quote_feed_result_ingress = lambda: True

    node = SimpleNamespace(
        get_event_loop=lambda: asyncio.get_running_loop(),
        kernel=SimpleNamespace(
            data_engine=SimpleNamespace(
                execute=lambda command: (_ for _ in ()).throw(
                    AssertionError(f"unexpected command={command!r}"),
                ),
                routing_map={feed_identity.instrument_id.venue: live_client},
            ),
            clock=SimpleNamespace(timestamp_ns=lambda: 123_456_789),
        ),
    )

    control_emitter: run_node.QuoteFeedControlEmitter | None = None

    def sink(command) -> None:
        run_node._emit_quote_feed_command(
            node=node,
            command=command,
            control_emitter=control_emitter,
        )

    control_emitter = run_node.QuoteFeedControlEmitter(
        node_scoped_id="aapl_tradexyz",
        sink=sink,
    )
    control_emitter.register_result_ingress(
        feed_identity,
        lambda **kwargs: ingress_calls.append(kwargs),
    )
    live_client._quote_feed_result_ingress = (
        lambda **kwargs: control_emitter.ingest_result(feed_identity, **kwargs)
    )

    control_emitter.reset(feed_identity)
    await asyncio.sleep(0)

    assert ingress_calls == [
        {
            "now_ns": 123_456_789,
            "ok": True,
            "error_summary": None,
            "instrument_id": feed_identity.instrument_id.value,
            "status": "replayed",
            "cache_refreshed": True,
            "result": dict(result_payload),
        },
    ]


def test_build_node_shared_recovery_preserves_real_strategy_on_quote_tick_delivery(
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

    maker_instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    reference_instrument_id = InstrumentId.from_str("AAPL.NASDAQ")

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=maker_instrument_id,
            reference_instrument_id=reference_instrument_id,
            data_clients={},
            exec_clients={},
            data_factories={},
            exec_factories={},
        ),
    )
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_profile_account_projection_feed", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_reference_balance_snapshot_provider", lambda **_kwargs: None)

    run_node.build_node(
        {
            "flux": {"namespace": "flux", "schema_version": "v1"},
            "identity": {
                "strategy_id": "aapl_tradexyz_maker",
                "external_strategy_id": "aapl_tradexyz_maker",
                "trader_id": "EQUITIES-LIVE-AAPL-TRADEXYZ-MAKER",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": False},
            "strategy": {
                "strategy_id": "aapl_tradexyz_maker",
                "param_set": "equities_maker",
                "order_qty": "1",
                "qty": "1",
            },
            "strategy_contracts": [
                {
                    "strategy_id": "aapl_tradexyz_maker",
                    "portfolio_asset_id": "AAPL",
                    "maker_instrument_id": str(maker_instrument_id),
                    "reference_instrument_id": str(reference_instrument_id),
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
    strategy._refresh_runtime_params_if_due = lambda **_kwargs: None
    strategy._retry_hedge_backlog = lambda **_kwargs: None
    strategy._refresh_maker_quotes = lambda **_kwargs: None
    strategy._publish_market_bbo = lambda **_kwargs: None
    strategy._publish_balances_if_due = lambda: None
    strategy._publish_state_snapshot = lambda **_kwargs: None

    strategy.on_quote_tick(
        SimpleNamespace(
            instrument_id=maker_instrument_id,
            bid_price=Decimal("190.00"),
            ask_price=Decimal("190.04"),
            ts_event=2_000_000_000,
        )
    )

    assert strategy._quote_feed_supervisor is not None
    assert strategy._quote_feed_control_emitter is not None
    assert strategy._latest_quotes[maker_instrument_id] == {
        "bid": Decimal("190.00"),
        "ask": Decimal("190.04"),
        "ts_ns": 2_000_000_000,
    }


def test_build_grouped_node_keeps_binance_perp_hooks_strategy_scoped(
    monkeypatch,
) -> None:
    captured: dict[str, object] = {"strategies": []}
    attach_calls = {"runtime": [], "inventory": [], "projection": [], "reference": []}
    maker_instrument_id = InstrumentId.from_str("AMZNUSDT-LINEAR.BINANCE")
    reference_instrument_id = InstrumentId.from_str("AMZN.NASDAQ")

    class _CapturedStrategy:
        def __init__(self, *, config) -> None:
            self.config = config

    class _CapturedNode:
        def __init__(self, config) -> None:
            captured["node_config"] = config
            self.trader = SimpleNamespace(
                add_strategy=lambda strategy: captured["strategies"].append(strategy),
            )

        def add_data_client_factory(self, _venue, _factory) -> None:
            return None

        def add_exec_client_factory(self, _venue, _factory) -> None:
            return None

        def build(self) -> None:
            captured["build_called"] = True

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_grouped_strategy_specs(monkeypatch, _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: SimpleNamespace(
            execution_instrument_id=maker_instrument_id,
            reference_instrument_id=reference_instrument_id,
            data_clients={},
            exec_clients={},
            data_factories={},
            exec_factories={},
        ),
    )
    monkeypatch.setattr(run_node, "_register_cash_borrowing_venues", lambda **_kwargs: None)
    monkeypatch.setattr(
        run_node,
        "_attach_runtime_params_manager",
        lambda **kwargs: attach_calls["runtime"].append(
            kwargs["strategy"].config.external_strategy_id,
        ),
    )
    monkeypatch.setattr(
        run_node,
        "_attach_portfolio_inventory_feed",
        lambda **kwargs: attach_calls["inventory"].append(
            kwargs["strategy"].config.external_strategy_id,
        ),
    )
    monkeypatch.setattr(
        run_node,
        "_attach_profile_account_projection_feed",
        lambda **kwargs: attach_calls["projection"].append(
            kwargs["strategy"].config.external_strategy_id,
        ),
    )
    monkeypatch.setattr(
        run_node,
        "_attach_reference_balance_snapshot_provider",
        lambda **kwargs: attach_calls["reference"].append(
            kwargs["strategy"].config.external_strategy_id,
        ),
    )

    run_node.build_grouped_node(
        (
            _split_equities_config(
                strategy_id="amzn_binance_perp_maker",
                param_set="equities_maker",
                trader_id="EQUITIES-LIVE-AMZN-BINANCE-PERP-MAKER",
                portfolio_asset_id="AMZN",
                maker_instrument_id=str(maker_instrument_id),
                reference_instrument_id=str(reference_instrument_id),
                execution_account_scope_id="binance.main",
            ),
            _split_equities_config(
                strategy_id="amzn_binance_perp_taker",
                param_set="equities_taker",
                trader_id="EQUITIES-LIVE-AMZN-BINANCE-PERP-TAKER",
                portfolio_asset_id="AMZN",
                maker_instrument_id=str(maker_instrument_id),
                reference_instrument_id=str(reference_instrument_id),
                execution_account_scope_id="binance.main",
            ),
        ),
        mode="live",
        force_enable_execution=False,
    )

    assert captured["build_called"] is True
    assert {
        strategy.config.external_strategy_id
        for strategy in captured["strategies"]
    } == {"amzn_binance_perp_maker", "amzn_binance_perp_taker"}
    assert str(captured["node_config"].trader_id) == "EQUITIES-LIVE-AMZN-BINANCE-PERP"
    assert captured["node_config"].message_bus.streams_prefix == (
        "flux:v1:in:stream:live:amzn_binance_perp"
    )
    assert attach_calls["runtime"] == [
        "amzn_binance_perp_maker",
        "amzn_binance_perp_taker",
    ]
    assert attach_calls["inventory"] == [
        "amzn_binance_perp_maker",
        "amzn_binance_perp_taker",
    ]
    assert attach_calls["projection"] == [
        "amzn_binance_perp_maker",
        "amzn_binance_perp_taker",
    ]
    assert attach_calls["reference"] == [
        "amzn_binance_perp_maker",
        "amzn_binance_perp_taker",
    ]


def test_build_grouped_node_rejects_duplicate_member_suffixes_for_node_group(
    monkeypatch,
) -> None:
    class _CapturedStrategy:
        def __init__(self, *, config) -> None:
            self.config = config

    _install_grouped_strategy_specs(monkeypatch, _CapturedStrategy)

    with pytest.raises(ValueError, match="duplicate maker member"):
        run_node.build_grouped_node(
            (
                _split_equities_config(
                    strategy_id="aapl_tradexyz_maker",
                    param_set="equities_maker",
                    trader_id="EQUITIES-LIVE-AAPL-TRADEXYZ-MAKER-A",
                    portfolio_asset_id="AAPL",
                    maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
                    reference_instrument_id="AAPL.NASDAQ",
                    execution_account_scope_id="hyperliquid.xyz.main",
                ),
                _split_equities_config(
                    strategy_id="aapl_tradexyz_maker",
                    param_set="equities_maker",
                    trader_id="EQUITIES-LIVE-AAPL-TRADEXYZ-MAKER-B",
                    portfolio_asset_id="AAPL",
                    maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
                    reference_instrument_id="AAPL.NASDAQ",
                    execution_account_scope_id="hyperliquid.xyz.main",
                ),
            ),
            mode="live",
            force_enable_execution=False,
        )


def test_build_grouped_node_rejects_divergent_node_scoped_config_for_node_group(
    monkeypatch,
) -> None:
    maker_instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    reference_instrument_id = InstrumentId.from_str("AAPL.NASDAQ")

    class _CapturedStrategy:
        def __init__(self, *, config) -> None:
            self.config = config

    maker_config = _split_equities_config(
        strategy_id="aapl_tradexyz_maker",
        param_set="equities_maker",
        trader_id="EQUITIES-LIVE-AAPL-TRADEXYZ-MAKER",
        portfolio_asset_id="AAPL",
        maker_instrument_id=str(maker_instrument_id),
        reference_instrument_id=str(reference_instrument_id),
        execution_account_scope_id="hyperliquid.xyz.main",
    )
    taker_config = _split_equities_config(
        strategy_id="aapl_tradexyz_taker",
        param_set="equities_taker",
        trader_id="EQUITIES-LIVE-AAPL-TRADEXYZ-TAKER",
        portfolio_asset_id="AAPL",
        maker_instrument_id=str(maker_instrument_id),
        reference_instrument_id=str(reference_instrument_id),
        execution_account_scope_id="hyperliquid.xyz.main",
    )
    maker_config["node"] = {
        "enable_execution": True,
        "venues": {"IBKR": {"ibg_client_id": 7}},
    }
    taker_config["node"] = {
        "enable_execution": True,
        "venues": {"IBKR": {"ibg_client_id": 7, "ibg_port": 4001}},
    }

    _install_grouped_strategy_specs(monkeypatch, _CapturedStrategy)
    with pytest.raises(ValueError, match="identical `node` tables"):
        run_node.build_grouped_node(
            (maker_config, taker_config),
            mode="live",
            force_enable_execution=False,
        )


def test_build_grouped_node_allows_sibling_specific_ibkr_client_ids(
    monkeypatch,
) -> None:
    maker_instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    reference_instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    captured: dict[str, object] = {"strategies": []}

    class _CapturedStrategy:
        def __init__(self, *, config) -> None:
            self.config = config

    class _CapturedNode:
        def __init__(self, config) -> None:
            captured["node_config"] = config
            self.trader = SimpleNamespace(
                add_strategy=lambda strategy: captured["strategies"].append(strategy),
            )

        def add_data_client_factory(self, _venue, _factory) -> None:
            return None

        def add_exec_client_factory(self, _venue, _factory) -> None:
            return None

        def build(self) -> None:
            captured["build_called"] = True

    maker_config = _split_equities_config(
        strategy_id="aapl_tradexyz_maker",
        param_set="equities_maker",
        trader_id="EQUITIES-LIVE-AAPL-TRADEXYZ-MAKER",
        portfolio_asset_id="AAPL",
        maker_instrument_id=str(maker_instrument_id),
        reference_instrument_id=str(reference_instrument_id),
        execution_account_scope_id="hyperliquid.xyz.main",
    )
    taker_config = _split_equities_config(
        strategy_id="aapl_tradexyz_taker",
        param_set="equities_taker",
        trader_id="EQUITIES-LIVE-AAPL-TRADEXYZ-TAKER",
        portfolio_asset_id="AAPL",
        maker_instrument_id=str(maker_instrument_id),
        reference_instrument_id=str(reference_instrument_id),
        execution_account_scope_id="hyperliquid.xyz.main",
    )
    maker_config["node"] = {
        "enable_execution": True,
        "venues": {
            "HYPERLIQUID": {"instrument_id": str(maker_instrument_id), "execution": True},
            "IBKR": {"instrument_id": str(reference_instrument_id), "ibg_client_id": 7},
        },
    }
    taker_config["node"] = {
        "enable_execution": True,
        "venues": {
            "HYPERLIQUID": {"instrument_id": str(maker_instrument_id), "execution": True},
            "IBKR": {"instrument_id": str(reference_instrument_id), "ibg_client_id": 30},
        },
    }

    monkeypatch.setattr(run_node, "TradingNode", _CapturedNode)
    _install_grouped_strategy_specs(monkeypatch, _CapturedStrategy)
    monkeypatch.setattr(
        run_node,
        "resolve_strategy_venues",
        lambda **_kwargs: captured.update(
            {
                "resolved_strategy_id": _kwargs["config"]["identity"]["external_strategy_id"],
                "resolved_ibkr_client_id": _kwargs["config"]["node"]["venues"]["IBKR"][
                    "ibg_client_id"
                ],
            },
        )
        or SimpleNamespace(
            execution_instrument_id=maker_instrument_id,
            reference_instrument_id=reference_instrument_id,
            data_clients={},
            exec_clients={},
            data_factories={},
            exec_factories={},
        ),
    )
    monkeypatch.setattr(run_node, "_register_cash_borrowing_venues", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)
    monkeypatch.setattr(
        run_node,
        "_attach_profile_account_projection_feed",
        lambda **_kwargs: None,
    )
    monkeypatch.setattr(
        run_node,
        "_attach_reference_balance_snapshot_provider",
        lambda **_kwargs: None,
    )

    node = run_node.build_grouped_node(
        (taker_config, maker_config),
        mode="live",
        force_enable_execution=False,
    )

    assert node is not None
    assert captured["build_called"] is True
    assert captured["resolved_strategy_id"] == "aapl_tradexyz_maker"
    assert captured["resolved_ibkr_client_id"] == 7
    assert {
        strategy.config.external_strategy_id
        for strategy in captured["strategies"]
    } == {"aapl_tradexyz_maker", "aapl_tradexyz_taker"}


def test_main_accepts_multi_strategy_config_paths_for_node_group(monkeypatch, tmp_path: Path) -> None:
    config_paths = [
        tmp_path / "aapl_tradexyz_maker.toml",
        tmp_path / "aapl_tradexyz_taker.toml",
    ]
    shared_path = tmp_path / "shared.toml"
    for path in config_paths:
        path.write_text("[identity]\nstrategy_id='strategy_a'\n", encoding="utf-8")
    shared_path.write_text("[redis]\nhost='127.0.0.1'\n", encoding="utf-8")

    maker_config = _split_equities_config(
        strategy_id="aapl_tradexyz_maker",
        param_set="equities_maker",
        trader_id="EQUITIES-LIVE-AAPL-TRADEXYZ-MAKER",
        portfolio_asset_id="AAPL",
        maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
        reference_instrument_id="AAPL.NASDAQ",
        execution_account_scope_id="hyperliquid.xyz.main",
    )
    taker_config = _split_equities_config(
        strategy_id="aapl_tradexyz_taker",
        param_set="equities_taker",
        trader_id="EQUITIES-LIVE-AAPL-TRADEXYZ-TAKER",
        portfolio_asset_id="AAPL",
        maker_instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
        reference_instrument_id="AAPL.NASDAQ",
        execution_account_scope_id="hyperliquid.xyz.main",
    )
    loaded_configs = {
        config_paths[0]: maker_config,
        config_paths[1]: taker_config,
    }
    captured: dict[str, object] = {}

    class _Node:
        def run(self) -> None:
            captured["ran"] = True

        def dispose(self) -> None:
            captured["disposed"] = True

    @contextmanager
    def _noop_lock(_config):
        captured["lock_strategy_id"] = _config["identity"]["strategy_id"]
        captured["lock_trader_id"] = _config["identity"]["trader_id"]
        yield

    monkeypatch.setattr(
        "sys.argv",
        [
            "run_node.py",
            "--config",
            str(config_paths[0]),
            "--config",
            str(config_paths[1]),
            "--shared-config",
            str(shared_path),
        ],
    )
    monkeypatch.setattr(
        run_node,
        "_load_runtime_config",
        lambda path, shared_config_path=None: loaded_configs[path],
    )
    monkeypatch.setattr(run_node, "_resolve_mode", lambda *_args, **_kwargs: "live")
    def _build_grouped_node(configs, **_kwargs):
        captured["build_configs"] = tuple(configs)
        return _Node()

    monkeypatch.setattr(run_node, "build_grouped_node", _build_grouped_node)
    monkeypatch.setattr(run_node, "_strategy_startup_lock", _noop_lock)

    run_node.main()

    assert len(captured["build_configs"]) == 2
    assert captured["build_configs"][0]["identity"]["external_strategy_id"] == "aapl_tradexyz_maker"
    assert captured["build_configs"][1]["identity"]["external_strategy_id"] == "aapl_tradexyz_taker"
    assert captured["lock_strategy_id"] == "aapl_tradexyz"
    assert captured["lock_trader_id"] == "EQUITIES-LIVE-AAPL-TRADEXYZ"
    assert captured["ran"] is True
    assert captured["disposed"] is True


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
