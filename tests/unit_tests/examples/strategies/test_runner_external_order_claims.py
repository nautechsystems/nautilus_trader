from __future__ import annotations

from types import SimpleNamespace

import pytest

from flux.runners.equities import run_node as equities_run_node
from flux.runners.tokenmm import run_node as tokenmm_run_node
from flux.strategies.equities_maker import EquitiesMakerStrategyConfig
from flux.strategies.equities_taker import EquitiesTakerStrategyConfig
from nautilus_trader.model.identifiers import InstrumentId


class _CapturedStrategy:
    def __init__(self, *, config) -> None:
        self.config = config

    def set_params_manager_factory(self, _factory) -> None:
        return None

    def configure_portfolio_inventory_feed(self, **_kwargs) -> None:
        return None


class _CapturedNode:
    def __init__(self, config) -> None:
        self.config = config
        self.strategies: list[object] = []
        self.trader = SimpleNamespace(add_strategy=self._add_strategy)

    def _add_strategy(self, strategy) -> None:
        self.strategies.append(strategy)
        self.strategy = strategy

    def add_data_client_factory(self, _venue, _factory) -> None:
        return None

    def add_exec_client_factory(self, _venue, _factory) -> None:
        return None

    def build(self) -> None:
        return None


def _register_claims(strategies: list[object]) -> dict[InstrumentId, str]:
    owners: dict[InstrumentId, str] = {}
    for strategy in strategies:
        for instrument_id in getattr(strategy.config, "external_order_claims", []) or []:
            owner = getattr(strategy.config, "external_strategy_id", "")
            if instrument_id in owners:
                raise ValueError(
                    f"duplicate external-order claim for {instrument_id}: "
                    f"{owners[instrument_id]} vs {owner}",
                )
            owners[instrument_id] = owner
    return owners


def test_tokenmm_build_node_sets_external_order_claims(monkeypatch) -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT")
    captured: dict[str, object] = {}

    def _captured_node(config):
        node = _CapturedNode(config)
        captured["node"] = node
        return node

    monkeypatch.setattr(tokenmm_run_node, "TradingNode", _captured_node)
    monkeypatch.setattr(tokenmm_run_node, "MakerV3Strategy", _CapturedStrategy)
    monkeypatch.setattr(
        tokenmm_run_node,
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
    monkeypatch.setattr(tokenmm_run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(tokenmm_run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)

    tokenmm_run_node.build_node(
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

    strategy = captured["node"].strategy
    assert strategy.config.allowed_submit_instrument_ids == [maker_instrument_id]
    assert strategy.config.external_order_claims == [maker_instrument_id]


def test_equities_build_node_sets_external_order_claims(monkeypatch) -> None:
    maker_instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    reference_instrument_id = InstrumentId.from_str("MSFT.NASDAQ")
    captured: dict[str, object] = {}

    def _captured_node(config):
        node = _CapturedNode(config)
        captured["node"] = node
        return node

    monkeypatch.setattr(equities_run_node, "TradingNode", _captured_node)
    monkeypatch.setattr(equities_run_node, "MakerV3Strategy", _CapturedStrategy)
    monkeypatch.setattr(
        equities_run_node,
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
    monkeypatch.setattr(equities_run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(equities_run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)

    equities_run_node.build_node(
        {
            "flux": {"namespace": "flux", "schema_version": "v1"},
            "identity": {
                "strategy_id": "aapl_tradexyz_makerv4",
                "external_strategy_id": "aapl_tradexyz_makerv4",
                "trader_id": "EQUITIES-LIVE-NASDAQ",
            },
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "node": {"enable_execution": True},
            "strategy": {"strategy_id": "aapl_tradexyz_makerv4", "order_qty": "1000"},
        },
        mode="live",
        force_enable_execution=False,
    )

    strategy = captured["node"].strategy
    assert strategy.config.allowed_submit_instrument_ids == [
        maker_instrument_id,
        reference_instrument_id,
    ]
    assert strategy.config.external_order_claims == [
        maker_instrument_id,
        reference_instrument_id,
    ]


def test_equities_grouped_node_keeps_same_node_siblings_strategy_scoped(monkeypatch) -> None:
    maker_instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    reference_instrument_id = InstrumentId.from_str("AAPL.NASDAQ")
    captured: dict[str, object] = {}

    def _captured_node(config):
        node = _CapturedNode(config)
        captured["node"] = node
        return node

    def _install_grouped_strategy_specs() -> None:
        specs = {
            "equities_maker": SimpleNamespace(
                strategy_id="equities_maker",
                strategy_cls=_CapturedStrategy,
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
                strategy_cls=_CapturedStrategy,
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
            equities_run_node,
            "get_strategy_spec",
            lambda name: specs[name],
            raising=False,
        )
        monkeypatch.setattr(
            equities_run_node,
            "resolve_strategy_spec_for_strategy_id",
            lambda strategy_id, default=None: specs[
                "equities_taker" if strategy_id.endswith("_taker") else "equities_maker"
            ],
            raising=False,
        )

    monkeypatch.setattr(equities_run_node, "TradingNode", _captured_node)
    _install_grouped_strategy_specs()
    monkeypatch.setattr(
        equities_run_node,
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
    monkeypatch.setattr(equities_run_node, "_register_cash_borrowing_venues", lambda **_kwargs: None)
    monkeypatch.setattr(equities_run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(equities_run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)
    monkeypatch.setattr(
        equities_run_node,
        "_attach_profile_account_projection_feed",
        lambda **_kwargs: None,
    )
    monkeypatch.setattr(
        equities_run_node,
        "_attach_reference_balance_snapshot_provider",
        lambda **_kwargs: None,
    )

    equities_run_node.build_grouped_node(
        (
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
        ),
        mode="live",
        force_enable_execution=False,
    )

    strategies = captured["node"].strategies
    assert [strategy.config.external_strategy_id for strategy in strategies] == [
        "aapl_tradexyz_maker",
        "aapl_tradexyz_taker",
    ]
    assert _register_claims(strategies) == {}


def test_equities_grouped_node_leaves_stray_external_orders_unclaimed_for_node_group(
    monkeypatch,
) -> None:
    maker_instrument_id = InstrumentId.from_str("AMZNUSDT-LINEAR.BINANCE")
    reference_instrument_id = InstrumentId.from_str("AMZN.NASDAQ")
    captured: dict[str, object] = {}

    def _captured_node(config):
        node = _CapturedNode(config)
        captured["node"] = node
        return node

    specs = {
            "equities_maker": SimpleNamespace(
                strategy_id="equities_maker",
                strategy_cls=_CapturedStrategy,
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
                strategy_cls=_CapturedStrategy,
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
    monkeypatch.setattr(equities_run_node, "TradingNode", _captured_node)
    monkeypatch.setattr(
        equities_run_node,
        "get_strategy_spec",
        lambda name: specs[name],
        raising=False,
    )
    monkeypatch.setattr(
        equities_run_node,
        "resolve_strategy_spec_for_strategy_id",
        lambda strategy_id, default=None: specs[
            "equities_taker" if strategy_id.endswith("_taker") else "equities_maker"
        ],
        raising=False,
    )
    monkeypatch.setattr(
        equities_run_node,
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
    monkeypatch.setattr(equities_run_node, "_register_cash_borrowing_venues", lambda **_kwargs: None)
    monkeypatch.setattr(equities_run_node, "_attach_runtime_params_manager", lambda **_kwargs: None)
    monkeypatch.setattr(equities_run_node, "_attach_portfolio_inventory_feed", lambda **_kwargs: None)
    monkeypatch.setattr(
        equities_run_node,
        "_attach_profile_account_projection_feed",
        lambda **_kwargs: None,
    )
    monkeypatch.setattr(
        equities_run_node,
        "_attach_reference_balance_snapshot_provider",
        lambda **_kwargs: None,
    )

    equities_run_node.build_grouped_node(
        (
            {
                "flux": {"namespace": "flux", "schema_version": "v1"},
                "identity": {
                    "strategy_id": "amzn_binance_perp_maker",
                    "external_strategy_id": "amzn_binance_perp_maker",
                    "trader_id": "EQUITIES-LIVE-AMZN-BINANCE-PERP-MAKER",
                },
                "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
                "node": {"enable_execution": True},
                "strategy": {
                    "strategy_id": "amzn_binance_perp_maker",
                    "param_set": "equities_maker",
                    "order_qty": "1",
                },
                "strategy_contracts": [
                    {
                        "strategy_id": "amzn_binance_perp_maker",
                        "portfolio_asset_id": "AMZN",
                        "maker_instrument_id": "AMZNUSDT-LINEAR.BINANCE",
                        "reference_instrument_id": "AMZN.NASDAQ",
                        "execution_account_scope_id": "binance.main",
                        "reference_account_scope_id": "ibkr.reference.main",
                        "hedge_account_scope_id": "ibkr.hedge.main",
                    },
                    {
                        "strategy_id": "amzn_binance_perp_taker",
                        "portfolio_asset_id": "AMZN",
                        "maker_instrument_id": "AMZNUSDT-LINEAR.BINANCE",
                        "reference_instrument_id": "AMZN.NASDAQ",
                        "execution_account_scope_id": "binance.main",
                        "reference_account_scope_id": "ibkr.reference.main",
                        "hedge_account_scope_id": "ibkr.hedge.main",
                    },
                ],
            },
            {
                "flux": {"namespace": "flux", "schema_version": "v1"},
                "identity": {
                    "strategy_id": "amzn_binance_perp_taker",
                    "external_strategy_id": "amzn_binance_perp_taker",
                    "trader_id": "EQUITIES-LIVE-AMZN-BINANCE-PERP-TAKER",
                },
                "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
                "node": {"enable_execution": True},
                "strategy": {
                    "strategy_id": "amzn_binance_perp_taker",
                    "param_set": "equities_taker",
                    "order_qty": "1",
                },
                "strategy_contracts": [
                    {
                        "strategy_id": "amzn_binance_perp_maker",
                        "portfolio_asset_id": "AMZN",
                        "maker_instrument_id": "AMZNUSDT-LINEAR.BINANCE",
                        "reference_instrument_id": "AMZN.NASDAQ",
                        "execution_account_scope_id": "binance.main",
                        "reference_account_scope_id": "ibkr.reference.main",
                        "hedge_account_scope_id": "ibkr.hedge.main",
                    },
                    {
                        "strategy_id": "amzn_binance_perp_taker",
                        "portfolio_asset_id": "AMZN",
                        "maker_instrument_id": "AMZNUSDT-LINEAR.BINANCE",
                        "reference_instrument_id": "AMZN.NASDAQ",
                        "execution_account_scope_id": "binance.main",
                        "reference_account_scope_id": "ibkr.reference.main",
                        "hedge_account_scope_id": "ibkr.hedge.main",
                    },
                ],
            },
        ),
        mode="live",
        force_enable_execution=False,
    )

    strategies = captured["node"].strategies
    assert [strategy.config.external_strategy_id for strategy in strategies] == [
        "amzn_binance_perp_maker",
        "amzn_binance_perp_taker",
    ]
    assert _register_claims(strategies) == {}


def test_grouped_node_duplicate_external_order_claims_still_raise() -> None:
    shared_instrument_id = InstrumentId.from_str("xyz:AAPL-USD-PERP.HYPERLIQUID")
    strategies = [
        SimpleNamespace(
            config=SimpleNamespace(
                external_strategy_id="aapl_tradexyz_maker",
                external_order_claims=[shared_instrument_id],
            ),
        ),
        SimpleNamespace(
            config=SimpleNamespace(
                external_strategy_id="aapl_tradexyz_taker",
                external_order_claims=[shared_instrument_id],
            ),
        ),
    ]

    with pytest.raises(ValueError, match="duplicate external-order claim"):
        _register_claims(strategies)
