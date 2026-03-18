from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
import sys
from typing import Any

from flux.strategies.equities_maker.constants import EQUITIES_MAKER_PARAM_SET
from flux.strategies.equities_maker.constants import EQUITIES_MAKER_PROFILE_KEY
from flux.strategies.equities_maker.constants import EQUITIES_MAKER_STRATEGY_FAMILY
from flux.strategies.equities_maker.constants import EQUITIES_MAKER_STRATEGY_ID
from flux.strategies.equities_maker.constants import EQUITIES_MAKER_STRATEGY_VERSION
from flux.strategies.equities_maker.strategy import EquitiesMakerStrategy
from flux.strategies.equities_maker.strategy import EquitiesMakerStrategyConfig
from flux.strategies.shared.capabilities import FluxStrategyCapabilities
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
    capabilities: FluxStrategyCapabilities
    strategy_id_suffixes: tuple[str, ...] = field(default_factory=tuple)

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
    capabilities=FluxStrategyCapabilities(
        publishes_local_inventory=True,
        uses_profile_account_projection=True,
        supports_immediate_hedge=False,
    ),
)

MAKERV4_STRATEGY_SPEC = FluxStrategySpec(
    strategy_id=MAKERV4_STRATEGY_ID,
    strategy_cls=MakerV4Strategy,
    config_cls=MakerV4StrategyConfig,
    param_set=MAKERV4_PARAM_SET,
    strategy_family=MAKERV4_STRATEGY_FAMILY,
    strategy_version=MAKERV4_STRATEGY_VERSION,
    profile_key=MAKERV4_PROFILE_KEY,
    capabilities=FluxStrategyCapabilities(
        publishes_local_inventory=True,
        uses_profile_account_projection=True,
        supports_immediate_hedge=True,
    ),
)

EQUITIES_MAKER_STRATEGY_SPEC = FluxStrategySpec(
    strategy_id=EQUITIES_MAKER_STRATEGY_ID,
    strategy_cls=EquitiesMakerStrategy,
    config_cls=EquitiesMakerStrategyConfig,
    param_set=EQUITIES_MAKER_PARAM_SET,
    strategy_family=EQUITIES_MAKER_STRATEGY_FAMILY,
    strategy_version=EQUITIES_MAKER_STRATEGY_VERSION,
    profile_key=EQUITIES_MAKER_PROFILE_KEY,
    capabilities=FluxStrategyCapabilities(
        publishes_local_inventory=False,
        uses_profile_account_projection=True,
        supports_immediate_hedge=True,
    ),
    strategy_id_suffixes=("maker",),
)

_SPECS: tuple[FluxStrategySpec, ...] = (
    MAKERV3_STRATEGY_SPEC,
    MAKERV4_STRATEGY_SPEC,
    EQUITIES_MAKER_STRATEGY_SPEC,
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


def resolve_strategy_spec_for_strategy_id(
    strategy_id: Any,
    *,
    default: FluxStrategySpec | None = None,
) -> FluxStrategySpec:
    normalized = _normalize_name(strategy_id)
    if not normalized:
        if default is not None:
            return default
        raise ValueError(f"Unsupported flux strategy id: {strategy_id!r}")

    direct_match = _SPECS_BY_NAME.get(normalized)
    if direct_match is not None:
        return direct_match

    for spec in _SPECS:
        suffixes = {
            spec.strategy_id,
            spec.profile_key,
            *spec.strategy_id_suffixes,
        }
        if any(normalized.endswith(f"_{suffix}") for suffix in suffixes):
            return spec

    if default is not None:
        return default
    raise ValueError(f"Unsupported flux strategy id: {strategy_id!r}")


def get_strategy_identity(name: Any) -> FluxStrategyIdentity:
    return get_strategy_spec(name).identity


__all__ = [
    "FluxStrategyIdentity",
    "FluxStrategyCapabilities",
    "FluxStrategySpec",
    "EQUITIES_MAKER_STRATEGY_SPEC",
    "MAKERV3_STRATEGY_SPEC",
    "MAKERV4_STRATEGY_SPEC",
    "get_strategy_identity",
    "get_strategy_spec",
    "get_strategy_specs",
    "resolve_strategy_spec_for_strategy_id",
]
