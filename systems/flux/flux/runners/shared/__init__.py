from __future__ import annotations

import sys
from importlib import import_module
from typing import Any


_EXPORTS: dict[str, tuple[str, str]] = {
    "EQUITIES_DESCRIPTOR": (".strategy_set", "EQUITIES_DESCRIPTOR"),
    "TOKENMM_DESCRIPTOR": (".strategy_set", "TOKENMM_DESCRIPTOR"),
    "StrategySetDescriptor": (".strategy_set", "StrategySetDescriptor"),
    "build_profile_strategy_maps": (".strategy_set", "build_profile_strategy_maps"),
    "build_profile_summary": (".strategy_set", "build_profile_summary"),
    "get_strategy_set_descriptor": (".strategy_set", "get_strategy_set_descriptor"),
    "get_strategy_set_descriptors": (".strategy_set", "get_strategy_set_descriptors"),
    "normalize_profile": (".strategy_set", "normalize_profile"),
    "supported_profile_ids": (".strategy_set", "supported_profile_ids"),
    "strategy_startup_lock": (".bootstrap", "strategy_startup_lock"),
    "parse_required_strategy_ids": (".portfolio_runner", "parse_required_strategy_ids"),
    "parse_strategy_ids": (".portfolio_runner", "parse_strategy_ids"),
}


if __name__ == "flux.runners.shared":
    sys.modules.setdefault("nautilus_trader.flux.runners.shared", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.runners.shared":
    sys.modules.setdefault("flux.runners.shared", sys.modules[__name__])


def __getattr__(name: str) -> Any:
    try:
        module_name, attr_name = _EXPORTS[name]
    except KeyError as exc:
        raise AttributeError(f"module {__name__!r} has no attribute {name!r}") from exc

    module = import_module(module_name, package=__name__)
    value = getattr(module, attr_name)
    globals()[name] = value
    return value


__all__ = list(_EXPORTS)
