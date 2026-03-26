from __future__ import annotations

from types import SimpleNamespace

from flux.runners.equities import run_node as equities_run_node
from flux.runners.tokenmm import run_node as tokenmm_run_node
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
        self.trader = SimpleNamespace(add_strategy=lambda strategy: setattr(self, "strategy", strategy))

    def add_data_client_factory(self, _venue, _factory) -> None:
        return None

    def add_exec_client_factory(self, _venue, _factory) -> None:
        return None

    def build(self) -> None:
        return None


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
