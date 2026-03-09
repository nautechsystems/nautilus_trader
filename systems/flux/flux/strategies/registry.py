from __future__ import annotations

from dataclasses import dataclass
import sys
from typing import Any

from flux.strategies.makerv4.constants import MAKERV4_PARAM_SET
from flux.strategies.makerv4.constants import MAKERV4_PROFILE_KEY
from flux.strategies.makerv4.constants import MAKERV4_STRATEGY_FAMILY
from flux.strategies.makerv4.constants import MAKERV4_STRATEGY_ID
from flux.strategies.makerv4.constants import MAKERV4_STRATEGY_VERSION
from flux.strategies.makerv4.strategy import MakerV4Strategy
from flux.strategies.makerv4.strategy import MakerV4StrategyConfig
from flux.strategies.makerv3.strategy import MakerV3Strategy
from flux.strategies.makerv3.strategy import MakerV3StrategyConfig

_CURRENT_MODULE = sys.modules[__name__]

if __name__ == "flux.strategies.registry":
    sys.modules["nautilus_trader.flux.strategies.registry"] = _CURRENT_MODULE
elif __name__ == "nautilus_trader.flux.strategies.registry":
    sys.modules["flux.strategies.registry"] = _CURRENT_MODULE

strategies_pkg = sys.modules.get("flux.strategies")
if strategies_pkg is not None:
    setattr(strategies_pkg, "registry", _CURRENT_MODULE)

compat_strategies_pkg = sys.modules.get("nautilus_trader.flux.strategies")
if compat_strategies_pkg is not None:
    setattr(compat_strategies_pkg, "registry", _CURRENT_MODULE)


@dataclass(frozen=True, slots=True)
class FluxStrategyIdentity:
    strategy_id: str
    strategy_family: str
    strategy_version: str
    param_set: str
    profile_key: str


@dataclass(frozen=True, slots=True)
class FluxStrategySpec:
    strategy_id: str
    strategy_cls: type[Any]
    config_cls: type[Any]
    param_set: str
    strategy_family: str
    strategy_version: str
    profile_key: str

    @property
    def name(self) -> str:
        return self.strategy_id

    @property
    def identity(self) -> FluxStrategyIdentity:
        return FluxStrategyIdentity(
            strategy_id=self.strategy_id,
            strategy_family=self.strategy_family,
            strategy_version=self.strategy_version,
            param_set=self.param_set,
            profile_key=self.profile_key,
        )


MAKERV3_STRATEGY_SPEC = FluxStrategySpec(
    strategy_id="makerv3",
    strategy_cls=MakerV3Strategy,
    config_cls=MakerV3StrategyConfig,
    param_set="makerv3",
    strategy_family="maker_v3",
    strategy_version="v3",
    profile_key="maker_v3",
)

MAKERV4_STRATEGY_SPEC = FluxStrategySpec(
    strategy_id=MAKERV4_STRATEGY_ID,
    strategy_cls=MakerV4Strategy,
    config_cls=MakerV4StrategyConfig,
    param_set=MAKERV4_PARAM_SET,
    strategy_family=MAKERV4_STRATEGY_FAMILY,
    strategy_version=MAKERV4_STRATEGY_VERSION,
    profile_key=MAKERV4_PROFILE_KEY,
)

_SPECS: tuple[FluxStrategySpec, ...] = (
    MAKERV3_STRATEGY_SPEC,
    MAKERV4_STRATEGY_SPEC,
)
_SPECS_BY_NAME = {spec.strategy_id: spec for spec in _SPECS}


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


def get_strategy_identity(name: Any) -> FluxStrategyIdentity:
    return get_strategy_spec(name).identity


__all__ = [
    "FluxStrategyIdentity",
    "FluxStrategySpec",
    "MAKERV3_STRATEGY_SPEC",
    "MAKERV4_STRATEGY_SPEC",
    "get_strategy_identity",
    "get_strategy_spec",
    "get_strategy_specs",
]
