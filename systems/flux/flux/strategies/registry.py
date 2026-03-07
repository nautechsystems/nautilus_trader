from __future__ import annotations

from dataclasses import dataclass
import sys
from typing import Any

from flux.strategies.makerv3.strategy import MakerV3Strategy
from flux.strategies.makerv3.strategy import MakerV3StrategyConfig

if __name__ == "flux.strategies.registry":
    sys.modules.setdefault("nautilus_trader.flux.strategies.registry", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.strategies.registry":
    sys.modules.setdefault("flux.strategies.registry", sys.modules[__name__])


@dataclass(frozen=True, slots=True)
class FluxStrategySpec:
    name: str
    strategy_cls: type[Any]
    config_cls: type[Any]
    param_set: str
    strategy_family: str
    strategy_version: str


MAKERV3_STRATEGY_SPEC = FluxStrategySpec(
    name="makerv3",
    strategy_cls=MakerV3Strategy,
    config_cls=MakerV3StrategyConfig,
    param_set="makerv3",
    strategy_family="maker_v3",
    strategy_version="v3",
)

_SPECS: tuple[FluxStrategySpec, ...] = (MAKERV3_STRATEGY_SPEC,)
_SPECS_BY_NAME = {spec.name: spec for spec in _SPECS}


def _normalize_name(value: Any) -> str:
    if value is None:
        return ""
    return str(value).strip().lower()


def get_strategy_specs() -> tuple[FluxStrategySpec, ...]:
    return _SPECS


def get_strategy_spec(name: Any) -> FluxStrategySpec:
    normalized = _normalize_name(name)
    spec = _SPECS_BY_NAME.get(normalized)
    if spec is None:
        raise ValueError(f"Unsupported flux strategy param set: {normalized or name!r}")
    return spec


__all__ = [
    "FluxStrategySpec",
    "MAKERV3_STRATEGY_SPEC",
    "get_strategy_spec",
    "get_strategy_specs",
]
