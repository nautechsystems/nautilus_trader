from __future__ import annotations

from nautilus_trader.flux.strategies import MakerV4Strategy as MakerV4StrategyFromRoot
from nautilus_trader.flux.strategies import MakerV4StrategyConfig as MakerV4StrategyConfigFromRoot
from nautilus_trader.flux.strategies.makerv4 import MakerV4Strategy
from nautilus_trader.flux.strategies.makerv4 import MakerV4StrategyConfig
from nautilus_trader.flux.strategies.registry import get_strategy_spec


def test_canonical_strategy_exports_match_root_surface() -> None:
    assert MakerV4StrategyFromRoot is MakerV4Strategy
    assert MakerV4StrategyConfigFromRoot is MakerV4StrategyConfig


def test_registry_exports_makerv4_spec() -> None:
    spec = get_strategy_spec("makerv4")

    assert spec.strategy_id == "makerv4"
    assert spec.param_set == "makerv4"
    assert spec.strategy_family == "maker_v4"
    assert spec.strategy_version == "v4"
    assert spec.strategy_cls is MakerV4Strategy
    assert spec.config_cls is MakerV4StrategyConfig
