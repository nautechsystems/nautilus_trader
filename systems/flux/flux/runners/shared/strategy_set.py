from __future__ import annotations

from collections.abc import Callable
from collections.abc import Mapping
from dataclasses import dataclass
import sys
from typing import Any

if __name__ == "flux.runners.shared.strategy_set":
    sys.modules.setdefault("nautilus_trader.flux.runners.shared.strategy_set", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.runners.shared.strategy_set":
    sys.modules.setdefault("flux.runners.shared.strategy_set", sys.modules[__name__])


@dataclass(frozen=True, slots=True)
class StrategySetDescriptor:
    profile: str
    aliases: tuple[str, ...]
    base_path: str
    route_aliases: tuple[str, ...]
    strategy_ids_field: str
    required_strategy_ids_field: str
    default_portfolio_id: str
    env_prefix: str
    pulse_group_key: str
    lock_dir_name: str
    default_unscoped_api: bool = False
    allow_discovery_without_allowlist: bool = False


TOKENMM_DESCRIPTOR = StrategySetDescriptor(
    profile="tokenmm",
    aliases=("tokenmm", "tokenm"),
    base_path="/tokenmm",
    route_aliases=("/tokenm",),
    strategy_ids_field="tokenmm_strategy_ids",
    required_strategy_ids_field="tokenmm_required_strategy_ids",
    default_portfolio_id="tokenmm",
    env_prefix="TOKENMM",
    pulse_group_key="tokenmm",
    lock_dir_name="tokenmm-strategy-locks",
    default_unscoped_api=True,
    allow_discovery_without_allowlist=True,
)

EQUITIES_DESCRIPTOR = StrategySetDescriptor(
    profile="equities",
    aliases=("equities",),
    base_path="/equities",
    route_aliases=("/tokenm",),
    strategy_ids_field="equities_strategy_ids",
    required_strategy_ids_field="equities_required_strategy_ids",
    default_portfolio_id="equities",
    env_prefix="EQUITIES",
    pulse_group_key="equities",
    lock_dir_name="equities-strategy-locks",
)

_DESCRIPTORS: tuple[StrategySetDescriptor, ...] = (
    EQUITIES_DESCRIPTOR,
    TOKENMM_DESCRIPTOR,
)
_DESCRIPTORS_BY_PROFILE = {descriptor.profile: descriptor for descriptor in _DESCRIPTORS}
_DESCRIPTORS_BY_ALIAS = {
    alias: descriptor
    for descriptor in _DESCRIPTORS
    for alias in descriptor.aliases
}


def _normalize_text(value: Any) -> str:
    if value is None:
        return ""
    return str(value).strip().lower()


def get_strategy_set_descriptors() -> tuple[StrategySetDescriptor, ...]:
    return _DESCRIPTORS


def get_strategy_set_descriptor(profile_or_alias: Any) -> StrategySetDescriptor | None:
    return _DESCRIPTORS_BY_ALIAS.get(_normalize_text(profile_or_alias))


def normalize_profile(profile_or_alias: Any) -> str:
    text = _normalize_text(profile_or_alias)
    descriptor = get_strategy_set_descriptor(text)
    if descriptor is not None:
        return descriptor.profile
    return text


def supported_profile_ids() -> tuple[str, ...]:
    return tuple(sorted(_DESCRIPTORS_BY_PROFILE))


def _coerce_strategy_ids(
    raw_value: Any,
    *,
    field_name: str,
    validate_identifier: Callable[[str, str], str],
) -> list[str]:
    if raw_value is None:
        return []
    if not isinstance(raw_value, list):
        raise ValueError(f"`api.{field_name}` must be a TOML array of strategy IDs")

    out: list[str] = []
    seen: set[str] = set()
    for index, value in enumerate(raw_value):
        text = str(value).strip()
        if not text:
            continue
        strategy_id = validate_identifier(text, f"api.{field_name}[{index}]")
        if strategy_id in seen:
            continue
        seen.add(strategy_id)
        out.append(strategy_id)
    return out


def build_profile_strategy_maps(
    api_cfg: Mapping[str, Any],
    *,
    descriptor: StrategySetDescriptor,
    validate_identifier: Callable[[str, str], str],
) -> tuple[dict[str, list[str]], dict[str, list[str]]]:
    strategy_ids = _coerce_strategy_ids(
        api_cfg.get(descriptor.strategy_ids_field),
        field_name=descriptor.strategy_ids_field,
        validate_identifier=validate_identifier,
    )
    required_ids = _coerce_strategy_ids(
        api_cfg.get(descriptor.required_strategy_ids_field),
        field_name=descriptor.required_strategy_ids_field,
        validate_identifier=validate_identifier,
    )

    if not strategy_ids:
        raise ValueError(f"`api.{descriptor.strategy_ids_field}` must be a non-empty TOML array")

    if required_ids:
        strategy_id_set = set(strategy_ids)
        unknown = sorted(strategy_id for strategy_id in required_ids if strategy_id not in strategy_id_set)
        if unknown:
            raise ValueError(
                f"`api.{descriptor.required_strategy_ids_field}` must be a subset of "
                f"`api.{descriptor.strategy_ids_field}`; unknown={unknown}",
            )

    profile_strategy_map = {descriptor.profile: strategy_ids}
    profile_required_strategy_map = (
        {descriptor.profile: required_ids}
        if required_ids
        else {}
    )
    return profile_strategy_map, profile_required_strategy_map


def build_profile_summary(
    descriptor: StrategySetDescriptor,
    profile_strategy_map: Mapping[str, list[str]],
    profile_required_strategy_map: Mapping[str, list[str]],
) -> str:
    strategy_ids = list(profile_strategy_map.get(descriptor.profile, []))
    required_ids = list(profile_required_strategy_map.get(descriptor.profile, strategy_ids))
    return (
        f"profile={descriptor.profile} "
        f"{descriptor.profile}_strategy_count={len(strategy_ids)} "
        f"{descriptor.strategy_ids_field}={strategy_ids} "
        f"{descriptor.required_strategy_ids_field}={required_ids}"
    )
